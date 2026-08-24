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
use std::path::{Path, PathBuf};

use quarto_pandoc_types::ConfigValue;

use crate::document_profile::{DocumentProfile, ListingItemInfo};

/// Where an item's link points. See plan
/// `2026-08-24-listing-inline-contents.md` §D1.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemTarget {
    /// A project document: the rendered output is the link.
    Document {
        /// Project-relative source path (forward-slash separated,
        /// matching `DocumentProfile::source_path`).
        source_path: PathBuf,
        /// Rendered output href.
        output_href: String,
    },
    /// A literal href the author wrote (`path:` on an inline record
    /// that names no project document — a remote URL, a PDF, or a
    /// `.qmd` that does not resolve, which is Q-12-20's fallback).
    ///
    /// Emitted into the template exactly as written. Note this is
    /// *not* immune to `LinkRewriteTransform`: a dead `.qmd` literal
    /// still looks like an internal reference to it, so it may draw a
    /// second (Q-13-*) diagnostic about the same broken link. That is
    /// the honest report of a link the author asked for and does not
    /// exist — Q-12-20 explains the cause, the rewriter reports the
    /// symptom.
    Href(String),
    /// No link at all (an inline record without `path:`).
    None,
}

impl ItemTarget {
    pub fn document(source_path: impl Into<PathBuf>, output_href: impl Into<String>) -> Self {
        ItemTarget::Document {
            source_path: source_path.into(),
            output_href: output_href.into(),
        }
    }

    /// Project-relative source path — documents only.
    pub fn source_path(&self) -> Option<&Path> {
        match self {
            ItemTarget::Document { source_path, .. } => Some(source_path),
            _ => None,
        }
    }

    /// What a link should point at: the rendered output for a
    /// document, the literal for `Href`, nothing for `None`.
    pub fn href(&self) -> Option<&str> {
        match self {
            ItemTarget::Document { output_href, .. } => Some(output_href),
            ItemTarget::Href(href) => Some(href),
            ItemTarget::None => None,
        }
    }

    /// The value `path` exposes to `include:`/`exclude:` and `sort:`:
    /// the project-relative source path for a document, the literal
    /// href otherwise. Q1's `item.path` is the link either way, so
    /// filters written against Q1 keep working.
    pub fn filter_path(&self) -> Option<String> {
        match self {
            ItemTarget::Document { source_path, .. } => Some(source_path.display().to_string()),
            ItemTarget::Href(href) => Some(href.clone()),
            ItemTarget::None => None,
        }
    }

    /// Rendered output href — documents only. The key the L7
    /// post-render placeholders and the feed's sibling lookup use.
    pub fn output_href(&self) -> Option<&str> {
        match self {
            ItemTarget::Document { output_href, .. } => Some(output_href),
            _ => None,
        }
    }

    /// Display file name: the source file's name for a document, the
    /// last path segment (query and fragment stripped) for a literal
    /// href — Q1 fills `filename` from `basename(path)` either way.
    pub fn filename(&self) -> Option<String> {
        match self {
            ItemTarget::Document { source_path, .. } => source_path
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
                .map(String::from),
            ItemTarget::Href(href) => href
                .split(['?', '#'])
                .next()
                .unwrap_or("")
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .map(String::from),
            ItemTarget::None => None,
        }
    }
}

/// How an item came to exist. Drives the generate transform's
/// decisions (L7 placeholder gating, diagnostics wording) so they
/// are explicit rather than inferred from the target's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemOrigin {
    /// Matched by a `contents:` glob; hydrated from a `DocumentProfile`.
    Document,
    /// An inline `contents:` record with no document behind it.
    Record,
    /// An inline record whose `path:` named a project document; the
    /// record's fields were laid over the document's item.
    RecordOverDocument,
}

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
    /// Author-curated position from top-level `order:` front matter
    /// (via `DocumentProfile::order`). Primary key of the default
    /// listing sort (`order asc, title asc`, Q1 parity —
    /// bd-listing-declared-order-3ixcvc4o).
    pub order: Option<i32>,
    /// Where the item links. See [`ItemTarget`].
    pub target: ItemTarget,
    /// How the item came to exist. See [`ItemOrigin`].
    pub origin: ItemOrigin,
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
                .map_or_else(|| profile.source_path.display().to_string(), String::from)
        });

    let subtitle = li.subtitle.clone().or_else(|| profile.subtitle.clone());
    let description = li
        .description
        .clone()
        .or_else(|| profile.description.clone());
    // Front-matter `image:` values are document-relative (Q1
    // semantics). Rebase to project-relative here so every consumer
    // — the host-page template (which re-relativizes against the
    // host dir), the RSS feed builder (which joins with the project
    // dir), the copy intent — sees one convention. Remote/absolute
    // URLs and data: URIs pass through untouched. See bd-qv2lsab0.
    let image = li
        .image
        .clone()
        .or_else(|| profile.image.clone())
        .map(|img| rebase_image(&img, &profile.source_path));
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
        order: profile.order,
        target: ItemTarget::document(profile.source_path.clone(), profile.output_href.clone()),
        origin: ItemOrigin::Document,
        extra: li.extra.clone(),
    }
}

/// Rebase a document-relative image path onto the document's
/// project-relative directory, normalizing `.`/`..` segments.
/// Absolute URLs, `data:` URIs, and root-absolute paths pass
/// through unchanged.
fn rebase_image(src: &str, source_path: &Path) -> String {
    let dir = source_path
        .parent()
        .map(|d| {
            d.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(os) => os.to_str(),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    rebase_image_from_dir(src, &dir)
}

/// Rebase a relative image path onto a project-relative directory
/// (`""` for the project root), normalizing `.`/`..` segments.
/// Absolute URLs, `data:` URIs, and root-absolute paths pass through.
pub(crate) fn rebase_image_from_dir(src: &str, dir: &str) -> String {
    if super::helpers::is_external_src(src) {
        return src.to_string();
    }
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in src.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

pub(crate) fn join_authors(authors: &[String]) -> Option<String> {
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

    // Front-matter image paths rebase onto the document's directory
    // (project-relative), leaving remote/absolute values untouched.
    // See bd-qv2lsab0.
    #[test]
    fn hydration_rebases_relative_image_to_project_relative() {
        let li = ListingItemInfo {
            image: Some("cover.png".to_string()),
            ..ListingItemInfo::default()
        };
        let item = hydrate_item(&profile_with(li));
        assert_eq!(item.image.as_deref(), Some("posts/cover.png"));
    }

    #[test]
    fn hydration_image_rebase_normalizes_dotdot() {
        let li = ListingItemInfo {
            image: Some("../shared/cover.png".to_string()),
            ..ListingItemInfo::default()
        };
        let item = hydrate_item(&profile_with(li));
        assert_eq!(item.image.as_deref(), Some("shared/cover.png"));
    }

    #[test]
    fn hydration_image_passes_through_absolute_forms() {
        for src in [
            "https://example.com/x.png",
            "http://example.com/x.png",
            "data:image/png;base64,AAAA",
            "/site-absolute.png",
        ] {
            let li = ListingItemInfo {
                image: Some(src.to_string()),
                ..ListingItemInfo::default()
            };
            let item = hydrate_item(&profile_with(li));
            assert_eq!(item.image.as_deref(), Some(src));
        }
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

    #[test]
    fn target_document_exposes_source_and_href() {
        let t = ItemTarget::document("posts/foo.qmd", "posts/foo.html");
        assert_eq!(t.source_path(), Some(std::path::Path::new("posts/foo.qmd")));
        assert_eq!(t.href(), Some("posts/foo.html"));
        assert_eq!(t.output_href(), Some("posts/foo.html"));
        assert_eq!(t.filename().as_deref(), Some("foo.qmd"));
    }

    #[test]
    fn target_href_is_literal_with_segment_filename() {
        let t = ItemTarget::Href("https://example.com/docs/report.pdf?v=2#top".to_string());
        assert_eq!(t.source_path(), None);
        assert_eq!(
            t.href(),
            Some("https://example.com/docs/report.pdf?v=2#top")
        );
        assert_eq!(
            t.output_href(),
            None,
            "only documents have a rendered output"
        );
        assert_eq!(t.filename().as_deref(), Some("report.pdf"));
    }

    #[test]
    fn target_filter_path_is_source_for_documents_and_literal_for_hrefs() {
        assert_eq!(
            ItemTarget::document("posts/foo.qmd", "posts/foo.html")
                .filter_path()
                .as_deref(),
            Some("posts/foo.qmd")
        );
        assert_eq!(
            ItemTarget::Href("https://example.com/x".to_string())
                .filter_path()
                .as_deref(),
            Some("https://example.com/x")
        );
        assert_eq!(ItemTarget::None.filter_path(), None);
    }

    #[test]
    fn target_none_has_nothing() {
        let t = ItemTarget::None;
        assert_eq!(t.source_path(), None);
        assert_eq!(t.href(), None);
        assert_eq!(t.output_href(), None);
        assert_eq!(t.filename(), None);
    }

    #[test]
    fn hydrated_item_is_a_document_target() {
        let item = hydrate_item(&profile_with(ListingItemInfo::default()));
        assert_eq!(item.origin, ItemOrigin::Document);
        assert_eq!(
            item.target,
            ItemTarget::document("posts/foo.qmd", "posts/foo.html")
        );
    }

    #[test]
    fn rebase_image_from_dir_handles_root_and_dotdot() {
        assert_eq!(rebase_image_from_dir("cover.png", ""), "cover.png");
        assert_eq!(
            rebase_image_from_dir("cover.png", "posts"),
            "posts/cover.png"
        );
        assert_eq!(
            rebase_image_from_dir("../shared/x.png", "a/b"),
            "a/shared/x.png"
        );
        assert_eq!(rebase_image_from_dir("/site.png", "posts"), "/site.png");
    }
}
