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

/// Carries Typst-specific values to insert into the filter-params blob.
pub struct TypstFilterParamsContributor {
    pub brand: Option<Value>,
    pub available_fonts: Option<Vec<String>>,
    pub citation_location: Option<String>,
    pub reference_location: Option<String>,
    /// `ctx.project.dir`, the same absolute path passed to `typst compile
    /// --root` in `typst_compile.rs`. Typst treats any path starting with
    /// `/` in an `image()` call as rooted at `--root`, not at the real
    /// filesystem root — so Lua's `mediabag-dir`-derived absolute paths
    /// (needed for `io.open`, since Pandoc inherits an uncontrolled cwd)
    /// must be rebased against this before being embedded in `.typ`
    /// source. See `modules/mediabag.lua`'s `typst_root_relative`.
    pub root_dir: Option<std::path::PathBuf>,
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
        if let Some(location) = &self.citation_location {
            blob.insert(
                "citation-location".to_string(),
                Value::String(location.clone()),
            );
        }
        if let Some(location) = &self.reference_location {
            blob.insert(
                "reference-location".to_string(),
                Value::String(location.clone()),
            );
        }
        if let Some(root_dir) = &self.root_dir {
            blob.insert(
                "typst-root-dir".to_string(),
                Value::String(root_dir.to_string_lossy().into_owned()),
            );
        }
    }
}
