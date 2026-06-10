/*
 * website_title_prefix.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * AST transform: prefix the page's `pagetitle` with `website.title`.
 */

//! Combine the document's title with `website.title` to produce a
//! prefixed `<title>` for HTML output.
//!
//! After [`MetadataNormalizeTransform`] derives
//! `pagetitle = title` from the document title, this transform reads
//! `website.title` from the merged metadata and rewrites
//! `pagetitle` so the rendered `<title>` element reads e.g.
//! `Getting Started – Quarto Docs`.
//!
//! Algorithm (Phase 7 sub-plan §Decision 4):
//!
//! - **No `website.title`** → no-op.
//! - **`pagetitle != title`** (user / earlier transform set it
//!   explicitly) → no-op. Preserves intentional overrides.
//! - **`title == website.title`** → leave `pagetitle = title` (don't
//!   double up).
//! - **Both `title` and `website.title` present, distinct** →
//!   `pagetitle = format!("{title} – {website.title}")` with an
//!   en-dash separator (U+2013, matches Q1 typography).
//! - **No `title`, only `website.title`** → `pagetitle =
//!   website.title`. Q1 has a narrower home-page-only carve-out;
//!   the simpler "any untitled page falls back to the site title"
//!   rule is the Phase-7 default.
//!
//! Source: `external-sources/quarto-cli/src/project/types/website/website-shared.ts`
//! lines 90–115 (`computePageTitle`).
//!
//! [`MetadataNormalizeTransform`]: crate::transforms::MetadataNormalizeTransform

use quarto_pandoc_types::ConfigMapEntry;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::config_value::ConfigValueKind;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;

use crate::Result;
use crate::project::website_config::website_title;
use crate::render::RenderContext;
use crate::transform::AstTransform;

/// Pandoc-style en-dash separator between page title and site title.
///
/// Matches Q1 (`website-shared.ts:108`).
const TITLE_SEPARATOR: &str = " – ";

/// AST transform: combine `title` and `website.title` into a
/// site-aware `pagetitle`.
pub struct WebsiteTitlePrefixTransform;

impl WebsiteTitlePrefixTransform {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebsiteTitlePrefixTransform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for WebsiteTitlePrefixTransform {
    fn name(&self) -> &str {
        "website-title-prefix"
    }

    async fn transform(&self, ast: &mut Pandoc, _ctx: &mut RenderContext) -> Result<()> {
        apply_title_prefix(&mut ast.meta);
        Ok(())
    }
}

/// Apply the title-prefix algorithm to a merged metadata value.
///
/// Idempotent: running this twice on the same metadata produces the
/// same final `pagetitle` (the second run sees `pagetitle != title`
/// because of the en-dash, and short-circuits).
fn apply_title_prefix(meta: &mut ConfigValue) {
    let Some(site_title) = website_title(meta) else {
        return;
    };

    // Snapshot the source-info up front so we can borrow `meta`
    // mutably below.
    let source_info = meta.source_info.clone();

    let ConfigValueKind::Map(entries) = &mut meta.value else {
        return;
    };

    let title = find_plain_text(entries, "title");
    let pagetitle = find_plain_text(entries, "pagetitle");

    // Preserve an explicit pagetitle that diverges from `title`.
    // `MetadataNormalizeTransform` derives `pagetitle = title`, so
    // when `pagetitle == title` we know it was auto-derived and is
    // safe to overwrite. When `pagetitle != title`, treat it as
    // user-set and leave it alone.
    let is_derived_or_absent = match (&pagetitle, &title) {
        (None, _) => true,
        (Some(pt), Some(t)) => pt == t,
        (Some(_), None) => false,
    };
    if !is_derived_or_absent {
        return;
    }

    let new_pagetitle = match (title, &site_title) {
        (Some(t), st) if &t == st => t,
        (Some(t), st) => format!("{t}{TITLE_SEPARATOR}{st}"),
        (None, st) => st.clone(),
    };

    set_or_insert_string(entries, "pagetitle", new_pagetitle, source_info);
}

/// Find an entry by key and return its plain-text form, if any.
fn find_plain_text(entries: &[ConfigMapEntry], key: &str) -> Option<String> {
    entries
        .iter()
        .find(|e| e.key == key)
        .and_then(|e| e.value.as_plain_text())
}

/// Replace the value of an existing key with a string scalar, or
/// append a new entry if the key is absent.
fn set_or_insert_string(
    entries: &mut Vec<ConfigMapEntry>,
    key: &str,
    value: String,
    source_info: SourceInfo,
) {
    let new_value = ConfigValue::new_string(value, source_info.clone());
    if let Some(entry) = entries.iter_mut().find(|e| e.key == key) {
        entry.value = new_value;
    } else {
        entries.push(ConfigMapEntry {
            key: key.to_string(),
            key_source: source_info,
            value: new_value,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;

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

    fn pagetitle_of(meta: &ConfigValue) -> Option<String> {
        meta.get("pagetitle").and_then(|v| v.as_plain_text())
    }

    /// Plan test 7: no `website.title` → `pagetitle` unchanged.
    #[test]
    fn title_prefix_no_op_without_website_title() {
        let mut meta = map(vec![("title", s("Doc")), ("pagetitle", s("Doc"))]);
        apply_title_prefix(&mut meta);
        assert_eq!(pagetitle_of(&meta), Some("Doc".to_string()));
    }

    /// Plan test 8: doc title and site title both present, distinct
    /// → en-dash join.
    #[test]
    fn title_prefix_combines_doc_and_site_titles() {
        let mut meta = map(vec![
            ("title", s("Getting Started")),
            ("pagetitle", s("Getting Started")),
            ("website", map(vec![("title", s("Quarto Docs"))])),
        ]);
        apply_title_prefix(&mut meta);
        assert_eq!(
            pagetitle_of(&meta),
            Some("Getting Started – Quarto Docs".to_string())
        );
    }

    /// Plan test 9: doc title equals site title → keep single
    /// title, no `– X` doubling.
    #[test]
    fn title_prefix_skips_when_titles_equal() {
        let mut meta = map(vec![
            ("title", s("Quarto Docs")),
            ("pagetitle", s("Quarto Docs")),
            ("website", map(vec![("title", s("Quarto Docs"))])),
        ]);
        apply_title_prefix(&mut meta);
        assert_eq!(pagetitle_of(&meta), Some("Quarto Docs".to_string()));
    }

    /// Plan test 10: untitled page (no `title`, no `pagetitle`) +
    /// site title → `pagetitle = site title`.
    #[test]
    fn title_prefix_uses_website_title_for_untitled_page() {
        let mut meta = map(vec![("website", map(vec![("title", s("Quarto Docs"))]))]);
        apply_title_prefix(&mut meta);
        assert_eq!(pagetitle_of(&meta), Some("Quarto Docs".to_string()));
    }

    /// Plan test 11: an explicit `pagetitle` (different from
    /// `title`) survives. This is the "user / earlier transform set
    /// it" case.
    #[test]
    fn title_prefix_preserves_explicit_pagetitle() {
        let mut meta = map(vec![
            ("title", s("Doc")),
            ("pagetitle", s("Explicit")),
            ("website", map(vec![("title", s("Site"))])),
        ]);
        apply_title_prefix(&mut meta);
        assert_eq!(pagetitle_of(&meta), Some("Explicit".to_string()));
    }

    /// Plan test 12: a `MetadataNormalize`-derived `pagetitle`
    /// (equals `title`) is rewritten to the prefixed form.
    #[test]
    fn title_prefix_overrides_normalize_derived_pagetitle() {
        let mut meta = map(vec![
            ("title", s("Doc")),
            ("pagetitle", s("Doc")),
            ("website", map(vec![("title", s("Site"))])),
        ]);
        apply_title_prefix(&mut meta);
        assert_eq!(pagetitle_of(&meta), Some("Doc – Site".to_string()));
    }

    /// Idempotency: running twice yields the same result. The
    /// derived check (`pagetitle == title`) fails on the second run
    /// because `pagetitle` was rewritten — so the second run is a
    /// no-op, leaving the prefixed pagetitle in place.
    #[test]
    fn title_prefix_is_idempotent() {
        let mut meta = map(vec![
            ("title", s("Doc")),
            ("pagetitle", s("Doc")),
            ("website", map(vec![("title", s("Site"))])),
        ]);
        apply_title_prefix(&mut meta);
        apply_title_prefix(&mut meta);
        assert_eq!(pagetitle_of(&meta), Some("Doc – Site".to_string()));
    }

    /// Defensive: a non-map metadata is left alone.
    #[test]
    fn title_prefix_no_op_on_non_map_meta() {
        let mut meta = ConfigValue::null(SourceInfo::for_test());
        apply_title_prefix(&mut meta);
        // Just shouldn't panic; nothing to assert on the structure.
    }
}
