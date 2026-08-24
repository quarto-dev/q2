//! Parse and resolve `_brand.yml` brand configuration for Quarto.
//!
//! This crate is the Rust port of the data-model half of Quarto 1's
//! brand support: it owns the typed shape of `_brand.yml` plus the
//! color / font / logo resolution rules. SCSS generation lives in
//! `quarto-sass::brand_layer`; consumers that don't need styling
//! (e.g. favicon resolution, future Typst integration) can use this
//! crate without pulling in the SCSS dependency surface.
//!
//! See `claude-notes/plans/2026-05-20-brand-yml-support.md` for the
//! design and references to the Quarto 1 source we are porting from.

mod error;
mod resolve;
mod resolved;
mod split;
mod types;

pub use error::BrandError;
pub use resolved::ResolvedBrand;
pub use split::SplitBrand;
pub use types::{
    Brand, BrandColor, BrandColorLightDark, BrandColorValue, BrandDefaults, BrandFont,
    BrandFontFile, BrandFontFileEntry, BrandFontGoogle, BrandFontStyle, BrandFontSystem,
    BrandFontWeight, BrandFontWeightAtom, BrandLogo, BrandLogoExplicit, BrandLogoResource,
    BrandMeta, BrandMetaLink, BrandMetaName, BrandRef, BrandTypography, BrandTypographyOptions,
    LogoEntry, UnifiedBrand,
};

impl UnifiedBrand {
    /// Parse a `_brand.yml` document from a YAML string.
    ///
    /// Parsing always produces the **unified** form (color values may
    /// be plain strings or `{light:, dark:}` pairs); call
    /// [`UnifiedBrand::split`] to obtain the single-mode [`Brand`]s
    /// the rest of the pipeline consumes.
    pub fn from_yaml_str(yaml: &str) -> Result<Self, BrandError> {
        serde_yaml::from_str(yaml).map_err(BrandError::Parse)
    }
}
