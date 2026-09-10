//! Error type for brand parsing and resolution.

use thiserror::Error;

use crate::validate::BrandPath;

#[derive(Debug, Error)]
pub enum BrandError {
    /// YAML parse / deserialization failure (covers unknown fields too).
    #[error("failed to parse _brand.yml: {0}")]
    Parse(#[from] serde_yaml::Error),

    /// Color name aliasing formed a cycle longer than 100 steps.
    #[error("circular reference in _brand.yml color definitions: {chain}")]
    CircularColorReference { chain: String },

    /// A color reference resolved to a name that isn't a valid CSS color
    /// and isn't aliased in the palette.
    #[error("unknown color name in _brand.yml: {name}")]
    UnknownColorName { name: String },

    /// A `weight:` under `typography` is not a number in `100..=900`, a
    /// weight keyword, a list of those, or (where a range is allowed) a
    /// numeric range `N..M` with valid ends.
    ///
    /// `path` is the YAML path of the offending value
    /// (`typography.fonts[0].weight`), `value` its text as written, and
    /// `reason` the specific rule it broke. Raised by
    /// [`Brand::validate`](crate::Brand::validate).
    #[error("invalid font weight `{value}` at {path} in _brand.yml: {reason}")]
    InvalidFontWeight {
        path: BrandPath,
        value: String,
        reason: String,
    },
}
