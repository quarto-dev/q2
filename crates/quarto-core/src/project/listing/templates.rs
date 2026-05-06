/*
 * project/listing/templates.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Embedded built-in listing templates.
//!
//! The three built-in listing types (`default`, `grid`, `table`)
//! ship as `include_str!`-embedded doctemplate sources next to
//! this module. They're served via [`MemoryResolver`] so authors
//! can read them as the canonical reference (the file paths under
//! `templates/` are tracked in git) and so L8 custom templates
//! reuse the same names via partial-include.
//!
//! TODO(bd-0wyo): the `item-default.template` currently renders
//! only the curated field set; Q1's `otherFields` loop emits a
//! `<div class="metadata-value listing-<field>">…</div>` per
//! non-curated field. v1 default listing matches the curated
//! field set only.

use quarto_doctemplate::MemoryResolver;

use super::config::ListingType;

const LISTING_DEFAULT: &str = include_str!("templates/listing-default.template");
const LISTING_GRID: &str = include_str!("templates/listing-grid.template");
const LISTING_TABLE: &str = include_str!("templates/listing-table.template");
const ITEM_DEFAULT: &str = include_str!("templates/item-default.template");
const ITEM_GRID: &str = include_str!("templates/item-grid.template");
const ITEM_TABLE: &str = include_str!("templates/item-table.template");

/// Build the [`MemoryResolver`] carrying every embedded built-in
/// partial. Every L3 listing render constructs one of these and
/// chains it through [`super::super::super::project_listing_resolver_for`]
/// so a custom template (L8) can shadow built-in names by placing
/// a same-named file next to the host page.
pub fn builtins_resolver() -> MemoryResolver {
    MemoryResolver::with_partials([
        ("listing-default", LISTING_DEFAULT),
        ("listing-grid", LISTING_GRID),
        ("listing-table", LISTING_TABLE),
        ("item-default", ITEM_DEFAULT),
        ("item-grid", ITEM_GRID),
        ("item-table", ITEM_TABLE),
    ])
}

/// Source of the top-level template for a listing type. The render
/// transform compiles this string with the built-ins resolver
/// chain; the partial inside (`item-default()`, etc.) is loaded
/// from the resolver.
pub fn top_level_template_source(kind: ListingType) -> &'static str {
    match kind {
        ListingType::Default => LISTING_DEFAULT,
        ListingType::Grid => LISTING_GRID,
        ListingType::Table => LISTING_TABLE,
        // L8 custom templates land via a separate code path; if we
        // get here with `Custom`, the render transform has already
        // emitted Q-12-1 and downgraded to `Default`.
        ListingType::Custom => LISTING_DEFAULT,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_resolver_serves_all_six_partials() {
        use quarto_doctemplate::PartialResolver;
        use std::path::Path;
        let r = builtins_resolver();
        let dummy = Path::new("listing.template");
        for name in [
            "listing-default",
            "listing-grid",
            "listing-table",
            "item-default",
            "item-grid",
            "item-table",
        ] {
            let p = r.get_partial(name, dummy);
            assert!(p.is_some(), "partial {} not served", name);
            assert!(!p.unwrap().is_empty(), "partial {} is empty", name);
        }
    }
}
