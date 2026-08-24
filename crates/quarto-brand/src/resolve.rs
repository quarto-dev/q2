//! Color / font / logo resolution on a parsed `Brand`.
//!
//! These methods port Q1's `Brand` class behavior from
//! `external-sources/quarto-cli/src/core/brand/brand.ts`. Path-related
//! resolution that touches the filesystem stays out of this crate — we
//! deal in already-loaded `Brand` values only.

use std::collections::HashSet;

use crate::error::BrandError;
use crate::types::{
    Brand, BrandColor, BrandFont, BrandLogoResource, BrandTypographyOptions, LogoEntry,
};

/// Maximum number of color-resolution hops before we declare a cycle.
/// Matches Q1's `seenValues.size < 100` guard.
const MAX_COLOR_HOPS: usize = 100;

impl Brand {
    /// Resolve a color name to a final CSS color value.
    ///
    /// Mirrors Q1's `Brand.getColor`:
    /// 1. If `name` is a palette key, recurse on the palette's value.
    /// 2. Else if `name` is a named theme color (`primary`, ...) and
    ///    that slot is set, recurse on its value.
    /// 3. Else: treat `name` as a CSS color and return it verbatim.
    ///
    /// Cycles are detected via a per-call seen-set; resolution longer
    /// than [`MAX_COLOR_HOPS`] is treated as a cycle.
    pub fn resolve_color(&self, name: &str) -> Result<String, BrandError> {
        self.resolve_color_inner(name, /* quiet = */ false)
    }

    /// Like [`resolve_color`] but never errors on unknown raw values.
    ///
    /// Q1 distinguishes a "quiet" mode used when a color reference is
    /// being checked against the format-specific name map and a miss
    /// just means "leave it alone". We don't emit warnings (no logger
    /// plumbed in yet); the practical difference today is that this
    /// variant returns an owned `String` directly without ever
    /// erroring on a missing palette entry.
    pub fn resolve_color_quiet(&self, name: &str) -> String {
        self.resolve_color_inner(name, /* quiet = */ true)
            .unwrap_or_else(|_| name.to_string())
    }

    fn resolve_color_inner(&self, name: &str, _quiet: bool) -> Result<String, BrandError> {
        let mut current = name.to_string();
        let mut seen: HashSet<String> = HashSet::new();

        for _ in 0..MAX_COLOR_HOPS {
            if !seen.insert(current.clone()) {
                let chain = ordered_chain(&seen, &current);
                return Err(BrandError::CircularColorReference { chain });
            }

            // Step 1: palette aliasing.
            if let Some(next) = self
                .color
                .as_ref()
                .and_then(|c| c.palette.as_ref())
                .and_then(|p| p.get(&current))
            {
                current = next.clone();
                continue;
            }

            // Step 2: named theme color.
            if BrandColor::is_named_theme_color(&current)
                && let Some(next) = self.color.as_ref().and_then(|c| c.named(&current))
            {
                current = next.to_string();
                continue;
            }

            // Step 3: passthrough — treat as a raw CSS color value.
            return Ok(current);
        }

        // Exceeded MAX_COLOR_HOPS without finding a terminal value;
        // treat as a cycle.
        Err(BrandError::CircularColorReference {
            chain: format!("(>{MAX_COLOR_HOPS} hops starting from {name})"),
        })
    }
}

/// Produce a deterministic chain string for the error message.
///
/// `seen` is unordered, so we approximate Q1's `Array.from(seenValues).join(" -> ")`
/// by listing names alphabetically and appending the offending repeat.
fn ordered_chain(seen: &HashSet<String>, repeat: &str) -> String {
    let mut names: Vec<&str> = seen.iter().map(String::as_str).collect();
    names.sort_unstable();
    let mut out = names.join(" -> ");
    out.push_str(" -> ");
    out.push_str(repeat);
    out
}

// ── typography ──────────────────────────────────────────────────────

impl<V> Brand<V> {
    /// Lookup a font slot by name. Returns the options for `base`,
    /// `headings`, `link`, `monospace`, `monospace-inline`, or
    /// `monospace-block`. For `monospace-{inline,block}` see
    /// [`effective_monospace_inline`] / [`effective_monospace_block`]
    /// which apply Q1's merge-with-generic-monospace semantics.
    pub fn font_slot(&self, name: &str) -> Option<&BrandTypographyOptions<V>> {
        let t = self.typography.as_ref()?;
        match name {
            "base" => t.base.as_ref(),
            "headings" => t.headings.as_ref(),
            "link" => t.link.as_ref(),
            "monospace" => t.monospace.as_ref(),
            "monospace-inline" => t.monospace_inline.as_ref(),
            "monospace-block" => t.monospace_block.as_ref(),
            _ => None,
        }
    }

    /// All entries from `typography.fonts`, in source order.
    pub fn fonts(&self) -> &[BrandFont] {
        self.typography.as_ref().map_or(&[], |t| t.fonts.as_slice())
    }

    /// Effective `monospace-inline` options: a merge of `monospace`
    /// (as defaults) with `monospace-inline` (overriding) — matching
    /// Q1's `{ ...monospace, ...monospaceInline }` spread.
    pub fn effective_monospace_inline(&self) -> Option<BrandTypographyOptions<V>>
    where
        V: Clone,
    {
        merge_mono(
            self.font_slot("monospace"),
            self.font_slot("monospace-inline"),
        )
    }

    /// Effective `monospace-block` options: a merge of `monospace`
    /// (as defaults) with `monospace-block` (overriding).
    pub fn effective_monospace_block(&self) -> Option<BrandTypographyOptions<V>>
    where
        V: Clone,
    {
        merge_mono(
            self.font_slot("monospace"),
            self.font_slot("monospace-block"),
        )
    }
}

/// Merge two optional font-options objects with the second winning
/// per-field (Q1's spread semantics).
fn merge_mono<V: Clone>(
    base: Option<&BrandTypographyOptions<V>>,
    over: Option<&BrandTypographyOptions<V>>,
) -> Option<BrandTypographyOptions<V>> {
    match (base, over) {
        (None, None) => None,
        (Some(b), None) => Some(b.clone()),
        (None, Some(o)) => Some(o.clone()),
        (Some(b), Some(o)) => Some(BrandTypographyOptions {
            family: o.family.clone().or_else(|| b.family.clone()),
            size: o.size.clone().or_else(|| b.size.clone()),
            line_height: o.line_height.clone().or_else(|| b.line_height.clone()),
            weight: o.weight.clone().or_else(|| b.weight.clone()),
            style: o.style.clone().or_else(|| b.style.clone()),
            color: o.color.clone().or_else(|| b.color.clone()),
            background_color: o
                .background_color
                .clone()
                .or_else(|| b.background_color.clone()),
            decoration: o.decoration.clone().or_else(|| b.decoration.clone()),
        }),
    }
}

// ── logo ────────────────────────────────────────────────────────────

impl<V> Brand<V> {
    /// Lookup a logo by size keyword: `small`, `medium`, or `large`.
    pub fn logo(&self, name: &str) -> Option<&LogoEntry> {
        let l = self.logo.as_ref()?;
        match name {
            "small" => l.small.as_ref(),
            "medium" => l.medium.as_ref(),
            "large" => l.large.as_ref(),
            _ => None,
        }
    }

    /// Lookup a named extra image from `logo.images.*`.
    pub fn logo_image(&self, name: &str) -> Option<&BrandLogoResource> {
        self.logo.as_ref()?.images.as_ref()?.get(name)
    }

    /// Path of the favicon-best small logo. Returns `None` if no
    /// small logo is configured, or if `small` is a light/dark pair
    /// (caller decides which side to use).
    ///
    /// Mirrors Q1's `getFavicon(brand)` from `core/brand/brand.ts`.
    pub fn favicon(&self) -> Option<&str> {
        self.logo("small").and_then(|l| l.single_path())
    }
}
