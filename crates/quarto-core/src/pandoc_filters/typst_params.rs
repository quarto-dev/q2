//! `extractTypstFilterParams` (Q1's own name for the family,
//! `format-typst.ts`) — the typst-only [`FilterParamsContributor`] that
//! adds the `brand` key built by [`super::typst_brand::build_brand_param`]
//! and the `typst-available-fonts` key built by
//! `super::super::stage::stages::typst_compile::discover_available_typst_fonts`.
//!
//! Q1 sets `brand` **generically for every Pandoc format**
//! (`command/render/filters.ts:197`'s `[kBrand]: options.format.render[kBrand]`,
//! part of `quartoFilterParams`, not a per-format extra) from an
//! already-resolved `LightDarkBrand`. Q2 has not extended that generic path
//! to any Pandoc-hybrid format yet — docx/pptx get no `brand` key today.
//! This contributor supplies it for typst only, through the extension point
//! [`super::params::FilterParamsBuilder::with_contributor`] P7 already
//! designed for per-format extras. Widening `brand` into a core,
//! format-independent key (matching Q1's shape) is out of this plan's
//! scope — file it separately if docx/pptx brand support is ever wanted.
//!
//! `typst-available-fonts` is genuinely typst-specific (no other
//! Pandoc-hybrid format has an analogous font-fallback-filtering
//! workaround), so it has no such "should this be generic" question.

use serde_json::{Map, Value};

use super::params::FilterParamsContributor;

/// Carries the pre-built `brand`/`typst-available-fonts` param values (or
/// nothing, when unavailable) to insert into the filter-params blob.
pub struct TypstFilterParamsContributor {
    pub brand: Option<Value>,
    pub available_fonts: Option<Vec<String>>,
}

impl FilterParamsContributor for TypstFilterParamsContributor {
    fn contribute(&self, blob: &mut Map<String, Value>) {
        if let Some(brand) = &self.brand {
            blob.insert("brand".to_string(), brand.clone());
        }
        if let Some(fonts) = &self.available_fonts {
            blob.insert(
                "typst-available-fonts".to_string(),
                Value::Array(fonts.iter().cloned().map(Value::String).collect()),
            );
        }
    }
}
