/*
 * project/website_config.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Site-level config readers for `website.*` keys.
 */

//! Centralized readers for `website.*` keys in a merged metadata
//! [`ConfigValue`].
//!
//! Phase 7 has six call sites that read `website.title`,
//! `website.site-url`, or `website.favicon`:
//!
//! 1. `WebsiteTitlePrefixTransform` (per-page Pass-2 transform).
//! 2. `WebsiteFaviconTransform` (per-page Pass-2 transform).
//! 3. `WebsiteCanonicalUrlTransform` (per-page Pass-2 transform).
//! 4. `copy_favicon` (post-render — favicon file copy).
//! 5. `write_sitemap` (post-render — sitemap.xml emission, gates on
//!    site-url).
//! 6. `write_robots_txt` (post-render — robots.txt emission, gates on
//!    site-url).
//!
//! Centralizing the reads keeps the keys behind named functions so
//! the eventual nav-config-placement migration (`bd-n9dr`) is a
//! single-file edit, and so per-page transforms and post-render code
//! cannot drift on key names.
//!
//! All three readers accept a `&ConfigValue`. Per-page transforms
//! pass `&ast.meta` (post-`MetadataMergeStage`, contains the merged
//! project + document metadata); post-render code passes
//! `project.config.metadata.as_ref()?` (raw project YAML). Either
//! source has the same `website.<key>` shape.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-7.md` Decision 7.
//!
//! **Favicon (`bd-97yc`).** Sites 2 and 4 above no longer read
//! `website.favicon` directly — they call
//! [`resolved_website_favicon`], which layers the brand fallback,
//! leading-slash normalization, URL passthrough, and website-only
//! gating on top of the raw [`website_favicon`] read. That function is
//! the reason the `<link rel="icon">` and the file copy cannot drift
//! apart: there is one answer to "what is this site's favicon", and
//! both ask for it. See
//! `claude-notes/plans/2026-07-27-brand-aware-favicon-fallback.md`.

use quarto_pandoc_types::ConfigValue;

use super::{ProjectContext, ProjectKind};

/// Read `website.title` from a merged metadata value.
///
/// Returns the plain-text form (Pandoc-inline titles are flattened
/// to their text content), or `None` if the key is absent or the
/// metadata is not a map.
pub fn website_title(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "title"])
        .and_then(|v| v.as_plain_text())
}

/// Read `website.site-url` from a merged metadata value.
///
/// Trailing slashes are **not** stripped — callers strip when they
/// need to compose absolute URLs (e.g. sitemap and canonical-url).
/// This avoids surprising callers that want the verbatim user value.
pub fn website_site_url(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "site-url"])
        .and_then(|v| v.as_plain_text())
}

/// Read `website.description` from a merged metadata value.
///
/// Returns the plain-text form (Pandoc-inline descriptions are
/// flattened to their text content), or `None` if the key is absent
/// or the metadata is not a map. Consumed by the `llms.txt` index
/// header (bd-llms-txt-unimplemented-oih6z6j7).
pub fn website_description(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "description"])
        .and_then(|v| v.as_plain_text())
}

/// Read `website.llms-txt` from a merged metadata value.
///
/// `true` only for a literal boolean `true`; absent, `false`, and
/// non-boolean values all read as disabled
/// (bd-llms-txt-unimplemented-oih6z6j7).
pub fn website_llms_txt_enabled(meta: &ConfigValue) -> bool {
    meta.get_path(&["website", "llms-txt"])
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Read `website.favicon` from a merged metadata value.
///
/// Returns the favicon path as written by the user. **Does not
/// normalize** a leading `/` — callers should call
/// [`normalize_favicon_path`] when they need the project-relative
/// form.
pub fn website_favicon(meta: &ConfigValue) -> Option<String> {
    meta.get_path(&["website", "favicon"])
        .and_then(|v| v.as_plain_text())
}

/// Normalize a user-written favicon path to project-relative form.
///
/// Strips a leading `/` if present (Q1 takes the path verbatim into
/// `offset + "/" + favicon`; we treat a leading `/` as
/// "site-rooted" — equivalent to "project-root-relative" since the
/// site root *is* the project's output root).
///
/// Forward-slash form is preserved; this is a path *expression*,
/// not a filesystem path.
pub fn normalize_favicon_path(raw: &str) -> String {
    raw.strip_prefix('/').unwrap_or(raw).to_string()
}

/// The favicon a website page should point at, in the form its
/// consumers want: a **project-relative path**, or an **absolute URL**.
///
/// This is the single answer to "what is this site's favicon", shared
/// by the per-page `<link rel="icon">`
/// ([`crate::transforms::WebsiteFaviconTransform`]) and the post-render
/// file copy ([`super::website_post_render`]). Keeping one function
/// means the two cannot disagree about precedence, normalization, or
/// what counts as a URL.
///
/// Resolution order (`bd-97yc`, mirroring Q1's `website.ts:185-205`):
///
/// 1. An explicit `website.favicon`, used verbatim.
/// 2. Otherwise the project brand's **small** logo, rebased from the
///    brand's directory to project-relative form.
///
/// Returns `None` when neither supplies one, when the brand's small
/// logo is a light/dark pair ([`quarto_brand::Brand::favicon`] declines
/// to pick a side — bd-v5z8w), or when the resolved value is empty.
///
/// **Website projects only.** The brand fallback fires on the *absence*
/// of `website.favicon`, so unlike the other `website.*` readers it has
/// no key to gate itself on. Without the explicit project-kind check, a
/// default project using `_brand.yml` purely for theming would start
/// emitting a favicon. Q1 scopes its fallback the same way, by living
/// inside the website project type. An explicit `website.favicon` is
/// *not* gated — that key already means what it says.
///
/// The result may be a URL, which must never be resolved against the
/// filesystem or made page-relative. Callers check with
/// [`quarto_util::is_external_url`]; both do.
pub fn resolved_website_favicon(
    meta: &ConfigValue,
    project: &ProjectContext,
) -> Option<ResolvedFavicon> {
    let (raw, origin) = match website_favicon(meta) {
        Some(explicit) => (explicit, FaviconOrigin::WebsiteFavicon),
        None => {
            if project.config.project_kind != ProjectKind::Website {
                return None;
            }
            let brand = project.config.brand.as_ref()?;
            (
                brand.favicon_relative_to(&project.dir)?,
                FaviconOrigin::BrandLogo,
            )
        }
    };

    // A URL is already in its final form: no leading-slash
    // normalization (a protocol-relative `//host/x` would lose a
    // slash and become a site-rooted path).
    if quarto_util::is_external_url(&raw) {
        return Some(ResolvedFavicon { path: raw, origin });
    }

    let normalized = normalize_favicon_path(&raw);
    (!normalized.is_empty()).then_some(ResolvedFavicon {
        path: normalized,
        origin,
    })
}

/// A resolved favicon: where to point at it, and which piece of user
/// config asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFavicon {
    /// Project-relative path, or an absolute URL.
    pub path: String,
    /// Which config supplied it.
    pub origin: FaviconOrigin,
}

/// Which piece of user config a favicon came from.
///
/// Carried alongside the path so diagnostics can name the key the user
/// actually wrote. A project relying on the brand fallback has no
/// `website.favicon` anywhere, so blaming that key for a missing file
/// would send the reader hunting for something that isn't there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaviconOrigin {
    /// An explicit `website.favicon` in the project or document config.
    WebsiteFavicon,
    /// The project brand's small logo (`logo.small` in `_brand.yml`).
    BrandLogo,
}

impl FaviconOrigin {
    /// How to refer to this source in a user-facing message.
    pub fn describe(self) -> &'static str {
        match self {
            FaviconOrigin::WebsiteFavicon => "website.favicon",
            FaviconOrigin::BrandLogo => "the brand's logo.small",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_pandoc_types::inline::Inline;
    use quarto_source_map::SourceInfo;

    fn s(value: &str) -> ConfigValue {
        ConfigValue::new_string(value.to_string(), SourceInfo::for_test())
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        let entries = entries
            .into_iter()
            .map(|(k, v)| ConfigMapEntry {
                key: k.to_string(),
                key_source: SourceInfo::for_test(),
                value: v,
            })
            .collect();
        ConfigValue::new_map(entries, SourceInfo::for_test())
    }

    fn null() -> ConfigValue {
        ConfigValue::null(SourceInfo::for_test())
    }

    fn pandoc_inlines(text: &str) -> ConfigValue {
        let inlines = vec![Inline::Str(quarto_pandoc_types::inline::Str {
            text: text.to_string(),
            source_info: SourceInfo::for_test(),
        })];
        ConfigValue::new_inlines(inlines, SourceInfo::for_test())
    }

    /// Test 1 (plan §Tests / Unit tests — `website_config` helpers):
    /// `website.title` as a string scalar is returned verbatim.
    #[test]
    fn website_title_reads_string() {
        let meta = map(vec![("website", map(vec![("title", s("Site"))]))]);
        assert_eq!(website_title(&meta), Some("Site".to_string()));
    }

    /// Test 2: `website.title` as Pandoc inlines is returned as
    /// flattened plain text.
    #[test]
    fn website_title_reads_inlines_as_plain_text() {
        let meta = map(vec![(
            "website",
            map(vec![("title", pandoc_inlines("Site Title"))]),
        )]);
        assert_eq!(website_title(&meta), Some("Site Title".to_string()));
    }

    /// Test 3: missing `website.title` returns `None`.
    #[test]
    fn website_title_missing_returns_none() {
        let meta = map(vec![("title", s("Doc Title"))]);
        assert_eq!(website_title(&meta), None);
    }

    fn b(value: bool) -> ConfigValue {
        ConfigValue::new_bool(value, SourceInfo::for_test())
    }

    /// `website.description` reads strings and inlines as plain text,
    /// absent as `None` (llms.txt index header,
    /// bd-llms-txt-unimplemented-oih6z6j7).
    #[test]
    fn website_description_reads_string_and_inlines() {
        let meta = map(vec![("website", map(vec![("description", s("A site"))]))]);
        assert_eq!(website_description(&meta), Some("A site".to_string()));

        let meta = map(vec![(
            "website",
            map(vec![("description", pandoc_inlines("A site"))]),
        )]);
        assert_eq!(website_description(&meta), Some("A site".to_string()));

        let meta = map(vec![("website", map(vec![("title", s("Site"))]))]);
        assert_eq!(website_description(&meta), None);
    }

    /// `website.llms-txt` is enabled only by a literal `true`:
    /// absent, `false`, and non-boolean values all read as disabled.
    #[test]
    fn website_llms_txt_enabled_only_for_true() {
        let on = map(vec![("website", map(vec![("llms-txt", b(true))]))]);
        assert!(website_llms_txt_enabled(&on));

        let off = map(vec![("website", map(vec![("llms-txt", b(false))]))]);
        assert!(!website_llms_txt_enabled(&off));

        let absent = map(vec![("website", map(vec![("title", s("Site"))]))]);
        assert!(!website_llms_txt_enabled(&absent));

        let non_bool = map(vec![("website", map(vec![("llms-txt", s("yes"))]))]);
        assert!(!website_llms_txt_enabled(&non_bool));

        let null_v = map(vec![("website", map(vec![("llms-txt", null())]))]);
        assert!(!website_llms_txt_enabled(&null_v));
    }

    /// Test 4: `website.site-url` as a string is returned verbatim
    /// (trailing slashes preserved).
    #[test]
    fn website_site_url_reads_string() {
        let meta = map(vec![(
            "website",
            map(vec![("site-url", s("https://example.com/"))]),
        )]);
        assert_eq!(
            website_site_url(&meta),
            Some("https://example.com/".to_string())
        );
    }

    /// Test 5: `website.favicon` as a string is returned verbatim
    /// (no normalization at the helper level).
    #[test]
    fn website_favicon_reads_string() {
        let meta = map(vec![("website", map(vec![("favicon", s("favicon.ico"))]))]);
        assert_eq!(website_favicon(&meta), Some("favicon.ico".to_string()));
    }

    /// Test 6: a non-map `meta` (e.g. null, scalar) returns `None`
    /// from all three helpers without panicking.
    #[test]
    fn website_helpers_handle_non_map_meta() {
        let meta = null();
        assert_eq!(website_title(&meta), None);
        assert_eq!(website_site_url(&meta), None);
        assert_eq!(website_favicon(&meta), None);

        let scalar = s("just a string");
        assert_eq!(website_title(&scalar), None);
        assert_eq!(website_site_url(&scalar), None);
        assert_eq!(website_favicon(&scalar), None);
    }

    /// Open-question 4 (resolved): leading-slash favicon paths
    /// normalize to project-relative form.
    #[test]
    fn normalize_favicon_strips_leading_slash() {
        assert_eq!(normalize_favicon_path("/favicon.ico"), "favicon.ico");
        assert_eq!(
            normalize_favicon_path("/assets/favicon.png"),
            "assets/favicon.png"
        );
    }

    /// Normalization is a no-op when no leading slash present.
    #[test]
    fn normalize_favicon_no_op_for_relative_path() {
        assert_eq!(normalize_favicon_path("favicon.ico"), "favicon.ico");
        assert_eq!(
            normalize_favicon_path("assets/favicon.png"),
            "assets/favicon.png"
        );
    }

    /// `resolved_website_favicon` — precedence, the brand fallback,
    /// normalization, and project-kind gating (bd-97yc).
    mod resolved_favicon {
        use super::*;
        use crate::project::ProjectConfig;
        use quarto_brand::{ResolvedBrand, UnifiedBrand as Brand};
        use std::path::PathBuf;

        const PROJECT_DIR: &str = "/project";

        /// A project of `kind`, optionally carrying a brand whose
        /// `_brand.yml` lives at `<project>/<brand_subdir>`.
        fn project(
            kind: ProjectKind,
            brand_yaml: Option<&str>,
            brand_subdir: &str,
        ) -> ProjectContext {
            let dir = PathBuf::from(PROJECT_DIR);
            let brand = brand_yaml.map(|yaml| {
                let brand_dir = if brand_subdir.is_empty() {
                    dir.clone()
                } else {
                    dir.join(brand_subdir)
                };
                ResolvedBrand::new(
                    Brand::from_yaml_str(yaml)
                        .expect("parse brand")
                        .split()
                        .light,
                    Some(brand_dir),
                )
            });
            ProjectContext {
                dir: dir.clone(),
                config: ProjectConfig {
                    project_kind: kind,
                    brand,
                    ..Default::default()
                },
                is_single_file: false,
                files: vec![],
                output_dir: dir.join("_site"),
                ..Default::default()
            }
        }

        fn website_with_brand(yaml: &str, subdir: &str) -> ProjectContext {
            project(ProjectKind::Website, Some(yaml), subdir)
        }

        fn favicon_meta(path: &str) -> ConfigValue {
            map(vec![("website", map(vec![("favicon", s(path))]))])
        }

        /// Most tests care only about the resolved path; `origin` has
        /// its own tests below.
        fn favicon_path(meta: &ConfigValue, project: &ProjectContext) -> Option<String> {
            resolved_website_favicon(meta, project).map(|f| f.path)
        }

        const SMALL_LOGO: &str = "logo:\n  small: logo.png\n";

        // ── the explicit key ───────────────────────────────────────

        #[test]
        fn explicit_favicon_is_used() {
            let p = project(ProjectKind::Website, None, "");
            assert_eq!(
                favicon_path(&favicon_meta("favicon.ico"), &p),
                Some("favicon.ico".to_string())
            );
        }

        #[test]
        fn explicit_favicon_wins_over_brand() {
            let p = website_with_brand(SMALL_LOGO, "");
            assert_eq!(
                favicon_path(&favicon_meta("favicon.ico"), &p),
                Some("favicon.ico".to_string()),
                "the brand is a fallback, not an override"
            );
        }

        /// The explicit key is not project-kind gated — it already
        /// says what it means.
        #[test]
        fn explicit_favicon_works_on_a_default_project() {
            let p = project(ProjectKind::Default, None, "");
            assert_eq!(
                favicon_path(&favicon_meta("favicon.ico"), &p),
                Some("favicon.ico".to_string())
            );
        }

        #[test]
        fn leading_slash_is_normalized_away() {
            let p = project(ProjectKind::Website, None, "");
            assert_eq!(
                favicon_path(&favicon_meta("/favicon.ico"), &p),
                Some("favicon.ico".to_string())
            );
        }

        #[test]
        fn empty_favicon_yields_none() {
            let p = project(ProjectKind::Website, None, "");
            assert_eq!(favicon_path(&favicon_meta(""), &p), None);
            assert_eq!(favicon_path(&favicon_meta("/"), &p), None);
        }

        // ── the brand fallback ─────────────────────────────────────

        #[test]
        fn brand_small_logo_is_the_fallback() {
            let p = website_with_brand(SMALL_LOGO, "");
            assert_eq!(favicon_path(&map(vec![]), &p), Some("logo.png".to_string()));
        }

        #[test]
        fn brand_logo_is_rebased_from_the_brand_directory() {
            let p = website_with_brand(SMALL_LOGO, "_brand");
            assert_eq!(
                favicon_path(&map(vec![]), &p),
                Some("_brand/logo.png".to_string())
            );
        }

        #[test]
        fn no_brand_and_no_key_yields_none() {
            let p = project(ProjectKind::Website, None, "");
            assert_eq!(favicon_path(&map(vec![]), &p), None);
        }

        #[test]
        fn brand_without_a_small_logo_yields_none() {
            let p = website_with_brand("logo:\n  large: big.png\n", "");
            assert_eq!(favicon_path(&map(vec![]), &p), None);
        }

        /// Picking a side of a light/dark pair is bd-v5z8w's job.
        #[test]
        fn brand_light_dark_small_logo_yields_none() {
            let p = website_with_brand("logo:\n  small:\n    light: l.png\n    dark: d.png\n", "");
            assert_eq!(favicon_path(&map(vec![]), &p), None);
        }

        // ── project-kind gating ────────────────────────────────────

        /// The fallback fires on the *absence* of `website.favicon`,
        /// so it needs an explicit gate — a default project that uses
        /// `_brand.yml` only for theming must not sprout a favicon.
        #[test]
        fn brand_fallback_is_website_only() {
            let p = project(ProjectKind::Default, Some(SMALL_LOGO), "");
            assert_eq!(favicon_path(&map(vec![]), &p), None);
        }

        // ── URLs ───────────────────────────────────────────────────

        #[test]
        fn brand_external_url_passes_through() {
            let p = website_with_brand("logo:\n  small: https://cdn.example.com/l.png\n", "_brand");
            assert_eq!(
                favicon_path(&map(vec![]), &p),
                Some("https://cdn.example.com/l.png".to_string()),
                "a URL must not be rebased against the brand directory"
            );
        }

        /// An explicit `website.favicon` URL takes the same path.
        /// Before bd-97yc this was mangled downstream: the value went
        /// through `page_url_for`, which produced
        /// `../https:/example.com/f.ico`.
        #[test]
        fn explicit_external_url_passes_through() {
            let p = project(ProjectKind::Website, None, "");
            assert_eq!(
                favicon_path(&favicon_meta("https://example.com/f.ico"), &p),
                Some("https://example.com/f.ico".to_string())
            );
        }

        /// A protocol-relative URL must not have its leading slash
        /// stripped by the site-root normalization — that would turn
        /// `//host/f.ico` into the site-rooted path `/host/f.ico`.
        #[test]
        fn protocol_relative_url_keeps_both_slashes() {
            let p = project(ProjectKind::Website, None, "");
            assert_eq!(
                favicon_path(&favicon_meta("//cdn.example.com/f.ico"), &p),
                Some("//cdn.example.com/f.ico".to_string())
            );
        }

        // ── origin ─────────────────────────────────────────────────

        /// `origin` exists so a missing-file warning can name the
        /// config the user actually wrote. A project on the fallback
        /// has no `website.favicon` to point at.
        #[test]
        fn origin_distinguishes_the_two_sources() {
            let explicit = project(ProjectKind::Website, None, "");
            assert_eq!(
                resolved_website_favicon(&favicon_meta("favicon.ico"), &explicit).map(|f| f.origin),
                Some(FaviconOrigin::WebsiteFavicon)
            );

            let fallback = website_with_brand(SMALL_LOGO, "");
            assert_eq!(
                resolved_website_favicon(&map(vec![]), &fallback).map(|f| f.origin),
                Some(FaviconOrigin::BrandLogo)
            );

            // Precedence also decides the origin.
            let both = website_with_brand(SMALL_LOGO, "");
            assert_eq!(
                resolved_website_favicon(&favicon_meta("favicon.ico"), &both).map(|f| f.origin),
                Some(FaviconOrigin::WebsiteFavicon)
            );
        }

        #[test]
        fn origin_descriptions_name_user_facing_config() {
            assert_eq!(FaviconOrigin::WebsiteFavicon.describe(), "website.favicon");
            assert_eq!(
                FaviconOrigin::BrandLogo.describe(),
                "the brand's logo.small"
            );
        }
    }
}
