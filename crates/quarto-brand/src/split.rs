//! Splitting a [`UnifiedBrand`] into its light and dark halves.
//!
//! Port of Q1's `splitUnifiedBrand` / `brandHasDarkMode`
//! (`external-sources/quarto-cli/src/core/brand/brand.ts:545,629`),
//! with one deliberate divergence: **logo entries are carried through
//! unsplit**. Q1's `splitLogo` collapses a `{light:, dark:}` logo pair
//! into a per-mode resource because its `Brand` class is per-mode
//! everywhere; q2's logo consumers (the favicon fallback bd-97yc, the
//! navbar brand image bd-hp3tx) run once per document and need both
//! sides to emit light/dark markup — which [`LogoEntry::LightDark`]
//! already models. Logo dark sides still count for
//! [`UnifiedBrand::has_dark_mode`] (Q1 parity).

use crate::types::{
    Brand, BrandColor, BrandColorValue, BrandTypography, BrandTypographyOptions, LogoEntry,
    UnifiedBrand, for_each_named_color_slot, for_each_typography_slot,
};

/// The result of [`UnifiedBrand::split`]: one single-mode [`Brand`]
/// per variant, plus Q1's `enablesDarkMode` flag.
///
/// Both halves always exist — for an all-plain brand they are equal —
/// and `enables_dark_mode` says whether the brand's *content* asks for
/// a dark variant (some value carries an explicit `dark:` side).
/// Consumers that compile a dark variant for other reasons (a declared
/// `theme: {light:, dark:}` pair) use `dark` regardless of the flag.
#[derive(Debug, Clone)]
pub struct SplitBrand {
    /// The light half: every pair collapsed to its `light:` side.
    pub light: Brand,
    /// The dark half: every pair collapsed to its `dark:` side. A
    /// pair with no `dark:` side leaves the slot absent in this half
    /// (Q1: no fallback to the light value).
    pub dark: Brand,
    /// Q1's `enablesDarkMode`: true iff some named color, typography
    /// color/background-color, or logo entry carries a `dark:` side.
    pub enables_dark_mode: bool,
}

impl UnifiedBrand {
    /// Q1's `brandHasDarkMode`: does any value in this brand carry an
    /// explicit `dark:` side? Plain strings and light-only pairs do
    /// not count.
    pub fn has_dark_mode(&self) -> bool {
        if let Some(c) = &self.color {
            macro_rules! any_color_dark {
                ($($field:ident),+) => {
                    $(
                        if c.$field.as_ref().is_some_and(BrandColorValue::has_dark) {
                            return true;
                        }
                    )+
                };
            }
            for_each_named_color_slot!(any_color_dark);
        }
        if let Some(t) = &self.typography {
            macro_rules! any_typography_dark {
                ($($field:ident),+) => {
                    $(
                        if let Some(opts) = &t.$field {
                            if opts.color.as_ref().is_some_and(BrandColorValue::has_dark)
                                || opts
                                    .background_color
                                    .as_ref()
                                    .is_some_and(BrandColorValue::has_dark)
                            {
                                return true;
                            }
                        }
                    )+
                };
            }
            for_each_typography_slot!(any_typography_dark);
        }
        if let Some(logo) = &self.logo {
            for entry in [&logo.small, &logo.medium, &logo.large]
                .into_iter()
                .flatten()
            {
                if matches!(entry, LogoEntry::LightDark { dark: Some(_), .. }) {
                    return true;
                }
            }
        }
        false
    }

    /// Split into single-mode halves (Q1's `splitUnifiedBrand`).
    ///
    /// - A plain string goes to **both** halves.
    /// - A `{light:, dark:}` pair sends each side to its half; a
    ///   missing side leaves the slot absent in that half.
    /// - `palette`, `meta`, `defaults`, `typography.fonts`, and all
    ///   non-color typography fields are shared.
    /// - Logo entries are carried through unchanged (see the module
    ///   docs for why this diverges from Q1's `splitLogo`).
    pub fn split(self) -> SplitBrand {
        let enables_dark_mode = self.has_dark_mode();
        let (light_color, dark_color) = match &self.color {
            Some(c) => {
                let (l, d) = split_color(c);
                (Some(l), Some(d))
            }
            None => (None, None),
        };
        let (light_typography, dark_typography) = match &self.typography {
            Some(t) => {
                let (l, d) = split_typography(t);
                (Some(l), Some(d))
            }
            None => (None, None),
        };
        let light = Brand {
            meta: self.meta.clone(),
            color: light_color,
            typography: light_typography,
            logo: self.logo.clone(),
            defaults: self.defaults.clone(),
        };
        let dark = Brand {
            meta: self.meta,
            color: dark_color,
            typography: dark_typography,
            logo: self.logo,
            defaults: self.defaults,
        };
        SplitBrand {
            light,
            dark,
            enables_dark_mode,
        }
    }
}

fn split_color(c: &BrandColor<BrandColorValue>) -> (BrandColor, BrandColor) {
    let mut light = BrandColor {
        palette: c.palette.clone(),
        ..Default::default()
    };
    let mut dark = BrandColor {
        palette: c.palette.clone(),
        ..Default::default()
    };
    macro_rules! split_slots {
        ($($field:ident),+) => {
            $(
                if let Some(v) = &c.$field {
                    light.$field = v.light().map(str::to_string);
                    dark.$field = v.dark().map(str::to_string);
                }
            )+
        };
    }
    for_each_named_color_slot!(split_slots);
    (light, dark)
}

fn split_typography(t: &BrandTypography<BrandColorValue>) -> (BrandTypography, BrandTypography) {
    let mut light = BrandTypography {
        fonts: t.fonts.clone(),
        ..Default::default()
    };
    let mut dark = BrandTypography {
        fonts: t.fonts.clone(),
        ..Default::default()
    };
    macro_rules! split_slots {
        ($($field:ident),+) => {
            $(
                if let Some(opts) = &t.$field {
                    let (l, d) = split_typography_options(opts);
                    light.$field = Some(l);
                    dark.$field = Some(d);
                }
            )+
        };
    }
    for_each_typography_slot!(split_slots);
    (light, dark)
}

fn split_typography_options(
    o: &BrandTypographyOptions<BrandColorValue>,
) -> (BrandTypographyOptions, BrandTypographyOptions) {
    let shared = BrandTypographyOptions {
        family: o.family.clone(),
        size: o.size.clone(),
        line_height: o.line_height.clone(),
        weight: o.weight.clone(),
        style: o.style.clone(),
        color: None,
        background_color: None,
        decoration: o.decoration.clone(),
    };
    let mut light = shared.clone();
    let mut dark = shared;
    light.color = o.color.as_ref().and_then(|v| v.light()).map(str::to_string);
    dark.color = o.color.as_ref().and_then(|v| v.dark()).map(str::to_string);
    light.background_color = o
        .background_color
        .as_ref()
        .and_then(|v| v.light())
        .map(str::to_string);
    dark.background_color = o
        .background_color
        .as_ref()
        .and_then(|v| v.dark())
        .map(str::to_string);
    (light, dark)
}
