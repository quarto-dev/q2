//! Post-parse validation of a brand (bd-5fseopxy).
//!
//! Parsing (`serde`) only checks the *shape* of `_brand.yml`. Some
//! values are shape-valid but meaningless — a `weight:` of `Bold`,
//! `700..400`, or a range on a typography slot — and Quarto 1's
//! behavior for these is a hard error (`Unknown font weight`). Quarto 2
//! once fell back to weight 400 silently; this module is the check that
//! makes such values an error carrying the YAML path of the offending
//! value, so the caller that still holds the source text can point a
//! diagnostic at the exact scalar.
//!
//! Validation is a separate step from parsing so that it can report a
//! path: serde's untagged enums lose the location of the value that
//! failed to match, and the brand's inline form (a `brand:` block in
//! `_quarto.yml`) never goes through a text parse at all.

use std::fmt;

use crate::BrandError;
use crate::types::{
    Brand, BrandFont, BrandFontFileEntry, BrandFontWeight, BrandFontWeightAtom,
    BrandFontWeightRange,
};

/// Lowest and highest weight accepted anywhere in a brand, per the
/// brand.yml specification (CSS itself allows 1–1000, but no brand
/// font source serves weights outside this span).
const MIN_WEIGHT: u32 = 100;
const MAX_WEIGHT: u32 = 900;

/// The weight keywords, in the order the diagnostic lists them.
const WEIGHT_KEYWORDS: &str = "thin, extra-light, ultra-light, light, normal, regular, medium, \
                               semi-bold, demi-bold, bold, extra-bold, ultra-bold, black";

/// Map a font-weight keyword to its numeric value.
///
/// The table is Quarto 1's `brandFontWeightValue`
/// (`core/sass/brand.ts`), itself the MDN common-weight-name mapping
/// minus 950. It is case-sensitive and closed: `Bold`, `semibold`,
/// `bolder`, and `lighter` all return `None`.
pub fn weight_name_to_number(name: &str) -> Option<u32> {
    Some(match name {
        "thin" => 100,
        "extra-light" | "ultra-light" => 200,
        "light" => 300,
        "normal" | "regular" => 400,
        "medium" => 500,
        "semi-bold" | "demi-bold" => 600,
        "bold" => 700,
        "extra-bold" | "ultra-bold" => 800,
        "black" => 900,
        _ => return None,
    })
}

/// One step of a [`BrandPath`]: a mapping key or a sequence index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrandPathSegment {
    Key(String),
    Index(usize),
}

/// The YAML path of a value inside a brand, e.g.
/// `typography.fonts[0].files[1].weight`.
///
/// Consumers that hold the brand's source (the `_brand.yml` text, or
/// the `ConfigValue` tree of an inline block) walk this path to find the
/// node's source location. `Display` renders the dotted/bracketed form
/// shown in diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrandPath(Vec<BrandPathSegment>);

impl BrandPath {
    pub fn segments(&self) -> &[BrandPathSegment] {
        &self.0
    }

    fn key(&self, k: &str) -> Self {
        let mut v = self.0.clone();
        v.push(BrandPathSegment::Key(k.to_string()));
        Self(v)
    }

    fn index(&self, i: usize) -> Self {
        let mut v = self.0.clone();
        v.push(BrandPathSegment::Index(i));
        Self(v)
    }
}

impl fmt::Display for BrandPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            match seg {
                BrandPathSegment::Key(k) => {
                    if i > 0 {
                        f.write_str(".")?;
                    }
                    f.write_str(k)?;
                }
                BrandPathSegment::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

impl<V> Brand<V> {
    /// Check every `weight:` under `typography` and return the first
    /// invalid one, in document order (`fonts[]` entries first, then
    /// the slots `base`, `headings`, `link`, `monospace`,
    /// `monospace-inline`, `monospace-block`).
    ///
    /// Rules:
    /// - numbers (alone, in a list, or as range ends) are in `100..=900`;
    /// - keywords are in the closed table of [`weight_name_to_number`];
    /// - a range `N..M` has `N <= M`, and is only allowed on
    ///   `typography.fonts[]` entries (google/bunny `weight`, and each
    ///   `files[].weight` of a file font) — a slot styles one kind of
    ///   text and takes a single weight.
    ///
    /// Callers that load a brand should run this right after parsing;
    /// the SCSS emitter also refuses invalid weights defensively, but
    /// only this check knows the YAML path a diagnostic can point at.
    pub fn validate(&self) -> Result<(), BrandError> {
        let Some(t) = self.typography.as_ref() else {
            return Ok(());
        };
        let root = BrandPath::default().key("typography");

        let fonts = root.key("fonts");
        for (i, font) in t.fonts.iter().enumerate() {
            let font_path = fonts.index(i);
            match font {
                BrandFont::Google(g) | BrandFont::Bunny(g) => {
                    if let Some(w) = &g.weight {
                        validate_weight(w, &font_path.key("weight"), RangeAllowed::Yes)?;
                    }
                }
                BrandFont::File(f) => {
                    let files = font_path.key("files");
                    for (j, entry) in f.files.iter().enumerate() {
                        if let BrandFontFileEntry::Explicit {
                            weight: Some(w), ..
                        } = entry
                        {
                            validate_weight(w, &files.index(j).key("weight"), RangeAllowed::Yes)?;
                        }
                    }
                }
                BrandFont::System(_) => {}
            }
        }

        let slots: [(&str, Option<&crate::BrandTypographyOptions<V>>); 6] = [
            ("base", t.base.as_ref()),
            ("headings", t.headings.as_ref()),
            ("link", t.link.as_ref()),
            ("monospace", t.monospace.as_ref()),
            ("monospace-inline", t.monospace_inline.as_ref()),
            ("monospace-block", t.monospace_block.as_ref()),
        ];
        for (name, slot) in slots {
            if let Some(w) = slot.and_then(|o| o.weight.as_ref()) {
                validate_weight(w, &root.key(name).key("weight"), RangeAllowed::No)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RangeAllowed {
    Yes,
    No,
}

fn invalid(path: &BrandPath, value: impl fmt::Display, reason: impl Into<String>) -> BrandError {
    BrandError::InvalidFontWeight {
        path: path.clone(),
        value: value.to_string(),
        reason: reason.into(),
    }
}

fn validate_weight(
    w: &BrandFontWeight,
    path: &BrandPath,
    range_allowed: RangeAllowed,
) -> Result<(), BrandError> {
    match w {
        BrandFontWeight::Number(n) => validate_number(*n, path, *n),
        BrandFontWeight::Range(r) => validate_range(*r, path, range_allowed),
        BrandFontWeight::Name(s) => validate_name(s, path),
        BrandFontWeight::List(items) => {
            for (i, atom) in items.iter().enumerate() {
                let item_path = path.index(i);
                match atom {
                    BrandFontWeightAtom::Number(n) => validate_number(*n, &item_path, *n)?,
                    BrandFontWeightAtom::Name(s) => validate_name(s, &item_path)?,
                }
            }
            Ok(())
        }
    }
}

fn validate_number(n: u32, path: &BrandPath, value: impl fmt::Display) -> Result<(), BrandError> {
    if (MIN_WEIGHT..=MAX_WEIGHT).contains(&n) {
        Ok(())
    } else {
        Err(invalid(
            path,
            value,
            format!("font weights must be between {MIN_WEIGHT} and {MAX_WEIGHT}"),
        ))
    }
}

fn validate_range(
    r: BrandFontWeightRange,
    path: &BrandPath,
    range_allowed: RangeAllowed,
) -> Result<(), BrandError> {
    if range_allowed == RangeAllowed::No {
        return Err(invalid(
            path,
            r,
            "a typography slot takes a single weight, not a range; declare the range on the \
             font under `typography.fonts` and give the slot one weight",
        ));
    }
    if r.min > r.max {
        return Err(invalid(
            path,
            r,
            format!(
                "the range minimum {} is greater than its maximum {}",
                r.min, r.max
            ),
        ));
    }
    if r.min < MIN_WEIGHT || r.max > MAX_WEIGHT {
        return Err(invalid(
            path,
            r,
            format!("range ends must be between {MIN_WEIGHT} and {MAX_WEIGHT}"),
        ));
    }
    Ok(())
}

/// A `Name` is either a quoted number (`"400"`), a keyword, or invalid.
fn validate_name(s: &str, path: &BrandPath) -> Result<(), BrandError> {
    if let Ok(n) = s.parse::<u32>() {
        return validate_number(n, path, s);
    }
    if weight_name_to_number(s).is_some() {
        return Ok(());
    }
    Err(invalid(
        path,
        s,
        format!(
            "not a weight keyword ({WEIGHT_KEYWORDS}), a number from {MIN_WEIGHT} to \
             {MAX_WEIGHT}, or a numeric range such as `400..700`"
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_display_uses_dots_and_brackets() {
        let p = BrandPath::default()
            .key("typography")
            .key("fonts")
            .index(0)
            .key("files")
            .index(1)
            .key("weight");
        assert_eq!(p.to_string(), "typography.fonts[0].files[1].weight");
        assert_eq!(BrandPath::default().to_string(), "");
    }

    #[test]
    fn range_from_str_is_syntax_only() {
        assert_eq!(
            "700..400".parse::<BrandFontWeightRange>(),
            Ok(BrandFontWeightRange { min: 700, max: 400 })
        );
        assert!("regular..bold".parse::<BrandFontWeightRange>().is_err());
        assert!("400..".parse::<BrandFontWeightRange>().is_err());
        assert!("..700".parse::<BrandFontWeightRange>().is_err());
        assert!("400...700".parse::<BrandFontWeightRange>().is_err());
        assert!("400..700..900".parse::<BrandFontWeightRange>().is_err());
        assert!("400".parse::<BrandFontWeightRange>().is_err());
    }
}
