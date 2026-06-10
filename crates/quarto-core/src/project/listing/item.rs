/*
 * project/listing/item.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Hydrated [`ListingItem`] — the structure the render transform
//! iterates over to build a per-listing template binding.
//!
//! Built at render time from a [`DocumentProfile`] (specifically
//! `profile.listing_item` plus the curated top-level fields). Not
//! stored on disk; built per host-page render.

use std::collections::BTreeMap;
use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;

use crate::document_profile::{DocumentProfile, ListingItemInfo};

/// One resolved listing item. See L2 §"Per-item: ListingItem".
#[derive(Debug, Clone, PartialEq)]
pub struct ListingItem {
    /// Display title. Hydration order:
    /// `listing_item.title → profile.title → filename stem`.
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    /// Author display string built by joining [`Self::authors`] with
    /// ", ". Templates that want each author separately read
    /// `authors`; templates that want a single rendered string read
    /// `author`.
    pub author: Option<String>,
    pub authors: Vec<String>,
    pub date: Option<String>,
    pub date_modified: Option<String>,
    pub categories: Vec<String>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub image_lazy_loading: Option<bool>,
    pub reading_time_minutes: Option<u32>,
    pub word_count: Option<u32>,
    /// Project-relative source path of the input file (forward-slash
    /// separated, matching `DocumentProfile::source_path`).
    pub source_path: PathBuf,
    /// Output href (the link target the template renders).
    pub output_href: String,
    /// Free-form fields pulled from `profile.listing_item.extra`.
    pub extra: BTreeMap<String, ConfigValue>,
}

/// Hydrate a [`ListingItem`] from a [`DocumentProfile`]. Falls back
/// through the curated chain documented on each field.
pub fn hydrate_item(profile: &DocumentProfile) -> ListingItem {
    let li: &ListingItemInfo = &profile.listing_item;

    let title = li
        .title
        .clone()
        .or_else(|| profile.title.clone())
        .unwrap_or_else(|| {
            profile
                .source_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
                .unwrap_or_else(|| profile.source_path.display().to_string())
        });

    let subtitle = li.subtitle.clone().or_else(|| profile.subtitle.clone());
    let description = li
        .description
        .clone()
        .or_else(|| profile.description.clone());
    let image = li.image.clone().or_else(|| profile.image.clone());
    let image_alt = li.image_alt.clone();
    let date = li.date.clone().or_else(|| profile.date.clone());
    // `date_modified` only lives on `listing_item` — there is no
    // top-level profile field for it (filesystem mtime is filled
    // into `listing_item.date_modified` by L1's auto-fill stage).
    let date_modified = li.date_modified.clone();

    // Categories: prefer `listing_item.categories` if set, falling
    // back to `profile.categories`. The full tag-aware merge using
    // `categories_raw` from both sides is performed by the
    // `MergedConfig` consumer in the generate transform when
    // resolving the host page's per-item view; the simple fallback
    // here is used by per-item code paths that don't invoke
    // MergedConfig.
    let categories = if !li.categories.is_empty() {
        li.categories.clone()
    } else {
        profile.categories.clone()
    };

    ListingItem {
        title,
        subtitle,
        description,
        author: join_authors(&profile.authors),
        authors: profile.authors.clone(),
        date,
        date_modified,
        categories,
        image,
        image_alt,
        image_lazy_loading: None,
        reading_time_minutes: li.reading_time_minutes,
        word_count: li.word_count,
        source_path: profile.source_path.clone(),
        output_href: profile.output_href.clone(),
        extra: li.extra.clone(),
    }
}

fn join_authors(authors: &[String]) -> Option<String> {
    if authors.is_empty() {
        return None;
    }
    Some(authors.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use std::path::PathBuf;

    fn profile_with(li: ListingItemInfo) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from("posts/foo.qmd"),
            output_href: "posts/foo.html".to_string(),
            format_id: "html".to_string(),
            title: Some("Top-level Title".to_string()),
            listing_item: li,
            ..DocumentProfile::default()
        }
    }

    // 15. hydration_falls_back_to_top_level_title
    #[test]
    fn hydration_falls_back_to_top_level_title() {
        let item = hydrate_item(&profile_with(ListingItemInfo::default()));
        assert_eq!(item.title, "Top-level Title");
    }

    // 16. hydration_uses_listing_item_title_override
    #[test]
    fn hydration_uses_listing_item_title_override() {
        let li = ListingItemInfo {
            title: Some("Listing Override".to_string()),
            ..ListingItemInfo::default()
        };
        let item = hydrate_item(&profile_with(li));
        assert_eq!(item.title, "Listing Override");
    }

    // hydration falls through to filename stem when no title at all
    #[test]
    fn hydration_falls_back_to_filename_stem() {
        let p = DocumentProfile {
            source_path: PathBuf::from("notes/2026-thoughts.qmd"),
            output_href: "notes/2026-thoughts.html".to_string(),
            format_id: "html".to_string(),
            title: None,
            ..DocumentProfile::default()
        };
        let item = hydrate_item(&p);
        assert_eq!(item.title, "2026-thoughts");
    }

    // 18. item_extra_present_in_binding (via hydrate_item — the
    // bridge to TemplateValue happens later in the render transform)
    #[test]
    fn item_extra_present_after_hydration() {
        use quarto_pandoc_types::ConfigValue;
        use quarto_source_map::SourceInfo;

        let mut extra = BTreeMap::new();
        extra.insert(
            "status".to_string(),
            ConfigValue::new_string("draft", SourceInfo::for_test()),
        );
        let li = ListingItemInfo {
            extra,
            ..ListingItemInfo::default()
        };
        let item = hydrate_item(&profile_with(li));
        assert_eq!(
            item.extra
                .get("status")
                .and_then(|v| v.as_plain_text())
                .as_deref(),
            Some("draft")
        );
    }

    #[test]
    fn hydration_joins_authors() {
        let p = DocumentProfile {
            source_path: PathBuf::from("posts/foo.qmd"),
            output_href: "posts/foo.html".to_string(),
            format_id: "html".to_string(),
            title: Some("X".to_string()),
            authors: vec!["Jane Doe".to_string(), "John Roe".to_string()],
            ..DocumentProfile::default()
        };
        let item = hydrate_item(&p);
        assert_eq!(item.author.as_deref(), Some("Jane Doe, John Roe"));
        assert_eq!(item.authors, vec!["Jane Doe", "John Roe"]);
    }
}
