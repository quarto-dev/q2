//! Bridge: `quarto_brand`'s [`ResolvedBrand`] into the `brand`
//! `QUARTO_FILTER_PARAMS` shape `typst-brand-yaml.lua` and
//! `filters/modules/brand/brand.lua` (both already vendored) expect.
//!
//! Q1's own shape is `Brand.processedData`
//! (`external-sources/quarto-cli/src/core/brand/brand.ts:44-53,72-162`):
//! `{ color: {<name>: <resolved css>}, typography: {<slot>: <raw
//! options>}, logo: {small?, medium?, large?, images: {}} }`, wrapped
//! per mode as `{ light: {...}, dark: {...} }` — the wire shape
//! `param('brand')` reads, then indexes by `brand-mode`
//! (`typst-brand-yaml.lua:61-63`).
//!
//! Colors are pre-resolved to final CSS values ([`Brand::resolve_color`])
//! because Q1's `processData` resolves them eagerly. Typography stays
//! **raw** (color fields keep the brand color *name*, not a resolved
//! CSS value) because `modules/brand/brand.lua`'s `get_typography`
//! (`:29-55`) does that resolution itself, per read, via `get_color`.
//!
//! One thing Q2's [`quarto_brand::SplitBrand`] does differently from
//! Q1's `splitUnifiedBrand`: logo entries are deliberately **not**
//! split per mode (see `quarto_brand::split` module docs) — a
//! `{light:, dark:}` logo pair survives into both the light and dark
//! `Brand` halves as the same [`LogoEntry::LightDark`]. Picking the
//! right side per mode is therefore this bridge's job, not something
//! it can inherit from the split.

use std::path::Path;

use quarto_brand::{Brand, BrandLogoResource, LogoEntry, ResolvedBrand};
use serde_json::{Map, Value, json};

/// Build the `brand` filter param from the light/dark halves of a
/// resolved, split brand.
///
/// Returns `None` when neither mode has a brand configured, matching
/// Q1's `param('brand')` being simply absent for a document with no
/// `brand:` key — callers should omit the `"brand"` key from the
/// filter-params blob entirely in that case, not insert `null`
/// (`typst-brand-yaml.lua`'s `brand and brand[brandMode]` guard treats
/// either the same, but omitting matches every other optional key in
/// this builder).
pub fn build_brand_param(
    light: Option<&ResolvedBrand>,
    dark: Option<&ResolvedBrand>,
    project_dir: &Path,
) -> Option<Value> {
    if light.is_none() && dark.is_none() {
        return None;
    }
    let mut obj = Map::new();
    if let Some(rb) = light {
        obj.insert(
            "light".to_string(),
            processed_data(rb, "light", project_dir),
        );
    }
    if let Some(rb) = dark {
        obj.insert("dark".to_string(), processed_data(rb, "dark", project_dir));
    }
    Some(Value::Object(obj))
}

/// One mode's `{ processedData: { color, typography, logo } }`.
fn processed_data(resolved: &ResolvedBrand, mode: &str, project_dir: &Path) -> Value {
    let brand = &resolved.brand;
    let prefix = resolved.path_prefix_relative_to(project_dir);

    json!({
        "processedData": {
            "color": color_map(brand),
            "typography": typography_map(brand),
            "logo": logo_map(brand, mode, &prefix),
        }
    })
}

/// `color: { <palette-key-or-slot-name>: <resolved css value> }` — Q1's
/// `processData`'s two loops (`brand.ts:73-82`) collapsed into one,
/// mirroring `quarto_sass::brand_layer::color_layer`'s same two loops.
fn color_map(brand: &Brand) -> Value {
    let mut colors = Map::new();
    if let Some(bc) = brand.color.as_ref() {
        if let Some(palette) = bc.palette.as_ref() {
            for key in palette.keys() {
                if let Ok(resolved) = brand.resolve_color(key) {
                    colors.insert(key.clone(), json!(resolved));
                }
            }
        }
        for (name, _) in bc.named_colors() {
            if let Ok(resolved) = brand.resolve_color(name) {
                colors.insert(name.to_string(), json!(resolved));
            }
        }
    }
    Value::Object(colors)
}

/// `typography: { <slot>: <raw BrandTypographyOptions> }` — one entry
/// per slot that resolves to something, `monospace-inline`/
/// `monospace-block` pre-merged with the generic `monospace` slot
/// (Q1's `{ ...monospace, ...monospaceInline }` spread, `brand.ts:126-142`,
/// already implemented by [`Brand::effective_monospace_inline`] /
/// [`Brand::effective_monospace_block`]).
fn typography_map(brand: &Brand) -> Value {
    let mut out = Map::new();
    let slots: [(&str, Option<quarto_brand::BrandTypographyOptions>); 6] = [
        ("base", brand.font_slot("base").cloned()),
        ("headings", brand.font_slot("headings").cloned()),
        ("link", brand.font_slot("link").cloned()),
        ("monospace", brand.font_slot("monospace").cloned()),
        ("monospace-inline", brand.effective_monospace_inline()),
        ("monospace-block", brand.effective_monospace_block()),
    ];
    for (key, opts) in slots {
        if let Some(opts) = opts {
            out.insert(
                key.to_string(),
                serde_json::to_value(opts).unwrap_or(Value::Null),
            );
        }
    }
    Value::Object(out)
}

/// `logo: { small?, medium?, large?, images: {} }`, each resource's
/// `path` rewritten relative to `project_dir` (Q1's `resolvePath`,
/// `brand.ts:247-265`), and a `{light:, dark:}` pair resolved down to
/// `mode`'s side (see the module docs — Q2's split deliberately leaves
/// this to the bridge rather than the split itself).
fn logo_map(brand: &Brand, mode: &str, prefix: &Path) -> Value {
    let mut out = Map::new();
    for (key, entry) in [
        ("small", brand.logo("small")),
        ("medium", brand.logo("medium")),
        ("large", brand.logo("large")),
    ] {
        if let Some(resource) = entry.and_then(|e| resource_for_mode(e, mode)) {
            out.insert(
                key.to_string(),
                resource_json(&resource.with_path_relative_to(prefix)),
            );
        }
    }
    let mut images = Map::new();
    if let Some(image_map) = brand.logo.as_ref().and_then(|l| l.images.as_ref()) {
        for (name, resource) in image_map {
            images.insert(
                name.clone(),
                resource_json(&resource.with_path_relative_to(prefix)),
            );
        }
    }
    out.insert("images".to_string(), Value::Object(images));
    Value::Object(out)
}

/// Picks `mode`'s side of a logo entry. A [`LogoEntry::Single`] serves
/// both modes identically; a missing side of a
/// [`LogoEntry::LightDark`] pair yields `None` for that mode (Q1
/// parity — no light/dark fallback, `brand.ts:624-628`'s `splitLogo`
/// destructures each side independently with no default).
fn resource_for_mode<'a>(entry: &'a LogoEntry, mode: &str) -> Option<&'a BrandLogoResource> {
    match entry {
        LogoEntry::Single(r) => Some(r),
        LogoEntry::LightDark { light, dark } => {
            if mode == "dark" {
                dark.as_ref()
            } else {
                light.as_ref()
            }
        }
    }
}

/// `{ path: <string>, alt: <string>? }` — `alt` omitted (not `null`)
/// when absent. Pandoc's Lua JSON bridge represents a JSON `null` as a
/// sentinel object, not Lua `nil`; `typst-brand-yaml.lua`'s
/// `quote_string(image.alt)` would stringify that sentinel into
/// literal garbage if the key were present with a `null` value, so
/// omission is the only safe encoding of "no alt text" here.
fn resource_json(resource: &BrandLogoResource) -> Value {
    let mut obj = Map::new();
    obj.insert("path".to_string(), json!(resource.path()));
    if let Some(alt) = resource.alt() {
        obj.insert("alt".to_string(), json!(alt));
    }
    Value::Object(obj)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_brand::UnifiedBrand;
    use std::path::PathBuf;

    /// Parse a single-mode `Brand` directly from a YAML fixture (no
    /// light/dark pairs) via the unified parser + split, taking the
    /// light half — sufficient for tests that don't exercise
    /// light/dark divergence.
    fn brand_from_yaml(yaml: &str) -> Brand {
        let unified = UnifiedBrand::from_yaml_str(yaml).expect("valid brand yaml");
        unified.split().light
    }

    fn resolved(brand: Brand, brand_dir: &str) -> ResolvedBrand {
        ResolvedBrand::new(brand, Some(PathBuf::from(brand_dir)))
    }

    /// T1: palette keys and named theme-color slots both resolve to
    /// final CSS values through the palette-aliasing chain — matching
    /// Q1's `processData` two-loop color construction.
    ///
    /// Revert hunk: reverting `color_map` to only walk `named_colors()`
    /// (dropping the palette loop) makes the `brand-blue` assertion RED.
    #[test]
    fn test_color_map_resolves_palette_and_named_slots() {
        let brand = brand_from_yaml(
            r##"
color:
  palette:
    brand-blue: "#1234ff"
  primary: brand-blue
  background: white
"##,
        );
        let colors = color_map(&brand);
        assert_eq!(colors["brand-blue"], json!("#1234ff"));
        assert_eq!(colors["primary"], json!("#1234ff"));
        assert_eq!(colors["background"], json!("white"));
    }

    /// T2: `monospace-inline` merges the generic `monospace` slot's
    /// `family` with its own `size`, and `color`/`background-color`
    /// stay as raw brand-color references (not pre-resolved) — the
    /// resolution Q1 defers to `get_typography` at read time.
    ///
    /// Revert hunk: swapping `effective_monospace_inline()` for a bare
    /// `font_slot("monospace-inline")` makes the `family` assertion RED
    /// (the merge is what pulls `family` in from `monospace`).
    #[test]
    fn test_typography_merges_monospace_and_keeps_colors_raw() {
        let brand = brand_from_yaml(
            r#"
color:
  primary: blue
typography:
  monospace:
    family: Fira Code
    color: primary
  monospace-inline:
    size: 0.9em
"#,
        );
        let typography = typography_map(&brand);
        let inline = &typography["monospace-inline"];
        assert_eq!(inline["family"], json!("Fira Code"));
        assert_eq!(inline["size"], json!("0.9em"));
        assert_eq!(
            inline["color"],
            json!("primary"),
            "color must stay a raw brand-color reference, not a resolved css value"
        );
    }

    /// T3: a logo path is rewritten relative to `project_dir` (brand
    /// file lives in `brand/`, so `brand/logo.png` — Q1's
    /// `resolvePath`), and a `{light:, dark:}` pair resolves to the
    /// requested mode's side only.
    ///
    /// Revert hunk: hardcoding `resource_for_mode` to always take the
    /// `light` side makes the dark-mode assertion RED.
    #[test]
    fn test_logo_map_resolves_paths_and_picks_mode() {
        let brand = brand_from_yaml(
            r#"
logo:
  small:
    light: light.png
    dark: dark.png
  images:
    foo: images/bar.png
"#,
        );
        let prefix = PathBuf::from("brand");

        let light = logo_map(&brand, "light", &prefix);
        assert_eq!(light["small"]["path"], json!("brand/light.png"));

        let dark = logo_map(&brand, "dark", &prefix);
        assert_eq!(dark["small"]["path"], json!("brand/dark.png"));

        assert_eq!(
            light["images"]["foo"]["path"],
            json!("brand/images/bar.png")
        );
    }

    /// T4: no brand configured on either side yields no `"brand"` key
    /// at all, not a `null`.
    ///
    /// Revert hunk: removing the early-return guard makes this RED
    /// (would return `Some(Value::Object({}))` instead of `None`).
    #[test]
    fn test_build_brand_param_absent_when_no_brand() {
        assert!(build_brand_param(None, None, Path::new("/project")).is_none());
    }

    /// T5: the top-level wrapper keys are exactly `light`/`dark`,
    /// matching `param('brand')[brandMode]`'s indexing.
    ///
    /// Revert hunk: renaming either wrapper key makes this RED.
    #[test]
    fn test_build_brand_param_wraps_light_and_dark_keys() {
        let light = resolved(
            brand_from_yaml("color:\n  primary: blue\n"),
            "project/brand",
        );
        let dark = resolved(
            brand_from_yaml("color:\n  primary: navy\n"),
            "project/brand",
        );
        let param = build_brand_param(Some(&light), Some(&dark), Path::new("project")).unwrap();
        assert_eq!(
            param["light"]["processedData"]["color"]["primary"],
            json!("blue")
        );
        assert_eq!(
            param["dark"]["processedData"]["color"]["primary"],
            json!("navy")
        );
    }
}
