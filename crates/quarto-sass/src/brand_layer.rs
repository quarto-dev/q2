//! Translate a parsed `Brand` (`_brand.yml`) into `SassLayer`s.
//!
//! Port of `external-sources/quarto-cli/src/core/sass/brand.ts`. The
//! output is one or more `SassLayer`s in the order Q1 produces them:
//!
//! 1. `defaults.bootstrap` layer (if `defaults.bootstrap` is set)
//! 2. Color layer (if `color` is set)
//! 3. Typography layer (if `typography` is set)
//!
//! The order matches Q1's `brandBootstrapSassLayers` (which `unshift`s
//! the bootstrap-defaults layer onto the front of the color/typography
//! layers produced by `brandSassLayers`).
//!
//! Two intentional deviations from Q1:
//! - Q1's `quarto-scss-analysis-annotation` comments are omitted; we
//!   don't have a Q2 analyzer that consumes them yet (tracked as a
//!   Phase 8 follow-up).
//! - Font-family values are wrapped in double quotes; Q1 emits them
//!   bare, which is fragile for multi-word names (`EB Garamond` would
//!   be parsed as a token list). The Q2 output is more robust.

use std::path::{Path, PathBuf};

use quarto_brand::{
    Brand, BrandFont, BrandFontFile, BrandFontFileEntry, BrandFontGoogle, BrandFontStyle,
    BrandFontWeight, BrandFontWeightAtom,
};

use crate::error::SassError;
use crate::types::SassLayer;

/// Bootstrap's named-color palette. A `color.palette` key whose name
/// matches one of these gets emitted into the bootstrap-defaults layer
/// as a plain `$<name>: <value> !default;` (no `brand-` prefix).
const BOOTSTRAP_COLOR_NAMES: &[&str] = &[
    "black", "white", "blue", "indigo", "purple", "pink", "red", "orange", "yellow", "green",
    "teal", "cyan",
];

/// Map from Bootstrap SCSS variable name → brand named-theme-color
/// slot. When the slot resolves to a value, the variable is emitted
/// alongside the resolved value. Mirrors Q1's `defaultColorNameMap` in
/// `core/sass/brand.ts`.
const DEFAULT_COLOR_NAME_MAP: &[(&str, &str)] = &[
    ("link-color", "link"),
    ("pre-color", "foreground"),
    ("body-bg", "background"),
    ("body-color", "foreground"),
    ("body-secondary-color", "secondary"),
    ("body-secondary", "secondary"),
    ("body-tertiary-color", "tertiary"),
    // Q1 has "secondary" here too; we keep that for parity.
    ("body-tertiary", "secondary"),
];

/// Translate a `Brand` into a vector of `SassLayer`s.
///
/// `font_path_prefix` is the directory path used to resolve relative
/// font-file URLs in `@font-face` blocks — it should be the brand
/// file's directory relative to the project root. For an empty path,
/// font files are referenced by their bare names.
///
/// Returns an empty vector if the brand has no color, typography, or
/// `defaults.bootstrap` content.
pub fn brand_to_layers(
    brand: &Brand,
    font_path_prefix: &Path,
) -> Result<Vec<SassLayer>, SassError> {
    let mut layers = Vec::new();

    // 1. bootstrap-defaults layer (only when defaults.bootstrap is
    //    set — matches Q1's `if (brand?.data?.defaults?.bootstrap)`
    //    guard before unshift).
    if let Some(bs) = bootstrap_defaults_layer(brand)? {
        layers.push(bs);
    }

    // 2. color layer
    if brand.color.is_some()
        && let Some(color) = color_layer(brand)?
    {
        layers.push(color);
    }

    // 3. typography layer
    if brand.typography.is_some()
        && let Some(typography) = typography_layer(brand, font_path_prefix)?
    {
        layers.push(typography);
    }

    Ok(layers)
}

// ── color layer ─────────────────────────────────────────────────────

fn color_layer(brand: &Brand) -> Result<Option<SassLayer>, SassError> {
    let Some(color) = brand.color.as_ref() else {
        return Ok(None);
    };

    let mut defaults: Vec<String> = vec!["/* color variables from _brand.yml */".to_string()];
    let mut rules: Vec<String> = vec![
        "/* color CSS variables from _brand.yml */".to_string(),
        ":root {".to_string(),
    ];

    // Brand palette → $brand-<name> and --brand-<name>.
    if let Some(palette) = color.palette.as_ref() {
        for key in palette.keys() {
            let var = sanitize_palette_key(key);
            let value = brand.resolve_color(key).map_err(brand_err)?;
            defaults.push(format!("$brand-{var}: {value} !default;"));
            rules.push(format!("  --brand-{var}: {value};"));
        }
    }

    // Named theme colors → $<name>: <resolved> !default;
    for (name, _) in color.named_colors() {
        let value = brand.resolve_color(name).map_err(brand_err)?;
        defaults.push(format!("${name}: {value} !default;"));
    }

    // Format-specific name map: emit $<sass-var>: <resolved> when the
    // mapped brand slot resolves to a non-identity value.
    for (sass_var, brand_slot) in DEFAULT_COLOR_NAME_MAP {
        let resolved = brand.resolve_color_quiet(brand_slot);
        if resolved != *brand_slot {
            defaults.push(format!("${sass_var}: {resolved} !default;"));
        }
    }

    rules.push("}".to_string());

    Ok(Some(SassLayer {
        uses: String::new(),
        defaults: defaults.join("\n"),
        functions: String::new(),
        mixins: String::new(),
        rules: rules.join("\n"),
    }))
}

/// Replace any run of non-`[A-Za-z0-9_-]` chars with a single `-`,
/// matching Q1's `colorKey.replace(/[^a-zA-Z0-9_-]+/g, "-")`.
fn sanitize_palette_key(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    let mut dash_pending = false;
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            if dash_pending {
                out.push('-');
                dash_pending = false;
            }
            out.push(ch);
        } else {
            dash_pending = true;
        }
    }
    if dash_pending {
        out.push('-');
    }
    out
}

// ── bootstrap-defaults layer ────────────────────────────────────────

fn bootstrap_defaults_layer(brand: &Brand) -> Result<Option<SassLayer>, SassError> {
    let Some(bootstrap) = brand.defaults.as_ref().and_then(|d| d.bootstrap()) else {
        return Ok(None);
    };

    let mut defaults: Vec<String> = vec!["/* Bootstrap defaults from _brand.yml */".to_string()];

    // Bootstrap colors from palette (only when defaults.bootstrap is
    // set, matching Q1's guard).
    if let Some(palette) = brand.color.as_ref().and_then(|c| c.palette.as_ref()) {
        for (key, _) in palette.iter() {
            if !BOOTSTRAP_COLOR_NAMES.contains(&key.as_str()) {
                continue;
            }
            let value = brand.resolve_color(key).map_err(brand_err)?;
            defaults.push(format!("${key}: {value} !default;"));
        }
    }

    // Read the typed shape of defaults.bootstrap. Q1 accepts either a
    // dict mapping SCSS var name → value, or a raw SCSS string. We
    // mirror that.
    let bs_defaults = bootstrap.get("defaults");
    if let Some(value) = bs_defaults {
        emit_bootstrap_defaults(value, &mut defaults)?;
    }

    let uses = extract_str_section(bootstrap, "uses");
    let functions = extract_str_section(bootstrap, "functions");
    let mixins = extract_str_section(bootstrap, "mixins");
    let rules = extract_str_section(bootstrap, "rules");

    Ok(Some(SassLayer {
        uses,
        defaults: defaults.join("\n"),
        functions,
        mixins,
        rules,
    }))
}

/// Pull a string-typed section (uses / functions / mixins / rules)
/// from `defaults.bootstrap`.
fn extract_str_section(bootstrap: &serde_yaml::Value, key: &str) -> String {
    bootstrap
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default()
}

/// Emit `$var: value !default;` lines for each entry in
/// `defaults.bootstrap.defaults`. Q1 accepts either a mapping or a raw
/// SCSS string.
fn emit_bootstrap_defaults(
    value: &serde_yaml::Value,
    out: &mut Vec<String>,
) -> Result<(), SassError> {
    if let Some(s) = value.as_str() {
        out.push(s.to_string());
        return Ok(());
    }
    if let Some(mapping) = value.as_mapping() {
        for (k, v) in mapping {
            let Some(key) = k.as_str() else {
                continue;
            };
            let val = yaml_scalar_to_scss(v);
            out.push(format!("${key}: {val} !default;"));
        }
        return Ok(());
    }
    Err(SassError::InvalidThemeConfig {
        message: "defaults.bootstrap.defaults must be a string or a mapping".to_string(),
        location: None,
    })
}

fn yaml_scalar_to_scss(v: &serde_yaml::Value) -> String {
    match v {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Number(n) => n.to_string(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Null => "null".to_string(),
        // For arrays/maps, fall back to a YAML repr — not great, but
        // matches Q1's permissiveness here.
        _ => serde_yaml::to_string(v)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

// ── typography layer ────────────────────────────────────────────────

fn typography_layer(
    brand: &Brand,
    font_path_prefix: &Path,
) -> Result<Option<SassLayer>, SassError> {
    if brand.typography.is_none() {
        return Ok(None);
    }

    // `uses` collects font imports + @font-face blocks (Q1 puts these
    // in the `uses` section, even though it's mis-named — see module
    // docstring).
    let mut import_lines: Vec<String> = Vec::new();
    let mut seen_imports: std::collections::HashSet<String> = std::collections::HashSet::new();

    for font in brand.fonts() {
        let line = match font {
            BrandFont::Google(g) => google_font_import_string(g),
            BrandFont::Bunny(b) => bunny_font_import_string(b),
            BrandFont::File(f) => file_font_face_block(f, font_path_prefix),
            BrandFont::System(_) => continue,
        };
        if seen_imports.insert(line.clone()) {
            import_lines.push(line);
        }
    }

    let mut defaults: Vec<String> = vec!["/* typography variables from _brand.yml */".to_string()];

    // Iterate kinds in the same order Q1 does — most specific first so
    // `!default` lets the specific value win when the same SCSS var is
    // targeted by multiple kinds (rare in practice but matches Q1).
    for kind in [
        "link",
        "monospace-block",
        "monospace-inline",
        "monospace",
        "headings",
        "base",
    ] {
        let Some(options) = brand.font_slot(kind) else {
            continue;
        };
        let translations = variable_translations_for_kind(kind);
        for (source, target) in translations {
            let value_str = match *source {
                "family" => options.family.as_deref().map(quote_family_name),
                "size" => options.size.as_deref().map(String::from),
                "line-height" => options.line_height.as_ref().map(yaml_scalar_to_scss),
                "weight" => options.weight.as_ref().map(font_weight_to_scss),
                "style" => options.style.as_ref().map(font_style_to_scss),
                "color" => options.color.as_ref().map(|c| brand.resolve_color_quiet(c)),
                "background-color" => options
                    .background_color
                    .as_ref()
                    .map(|c| brand.resolve_color_quiet(c)),
                "decoration" => options.decoration.as_deref().map(String::from),
                _ => None,
            };
            if let Some(v) = value_str {
                defaults.push(format!("${target}: {v} !default;"));
            }
        }
    }

    let uses = import_lines.join("\n");

    let layer = SassLayer {
        uses,
        defaults: defaults.join("\n"),
        functions: String::new(),
        mixins: String::new(),
        rules: String::new(),
    };
    if layer.is_empty() {
        return Ok(None);
    }
    Ok(Some(layer))
}

fn quote_family_name(family: &str) -> String {
    // SCSS will keep these as quoted strings into the compiled CSS,
    // which is what we want for multi-word family names. Single-word
    // names are also fine quoted.
    let escaped = family.replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn font_weight_to_scss(w: &BrandFontWeight) -> String {
    match w {
        BrandFontWeight::Number(n) => n.to_string(),
        BrandFontWeight::Name(s) => {
            weight_name_to_number(s).map_or_else(|| s.clone(), |n| n.to_string())
        }
        BrandFontWeight::List(items) => items
            .iter()
            .map(|a| match a {
                BrandFontWeightAtom::Number(n) => n.to_string(),
                BrandFontWeightAtom::Name(s) => {
                    weight_name_to_number(s).map_or_else(|| s.clone(), |n| n.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Map a font-weight keyword to its numeric value, matching Q1's
/// `brandFontWeightValue` table.
fn weight_name_to_number(name: &str) -> Option<u32> {
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

fn font_style_to_scss(s: &BrandFontStyle) -> String {
    match s {
        BrandFontStyle::One(s) => s.clone(),
        BrandFontStyle::List(v) => v.join(", "),
    }
}

fn variable_translations_for_kind(kind: &str) -> &'static [(&'static str, &'static str)] {
    match kind {
        "base" => &[
            // bootstrap
            ("family", "font-family-base"),
            ("size", "font-size-base"),
            ("line-height", "line-height-base"),
            ("weight", "font-weight-base"),
            // revealjs (reveal.js 6 uses kebab-case Sass vars; the Quarto reveal
            // layer maps $font-family-sans-serif → $main-font, and reads
            // $main-font directly, so target the reveal-6 name)
            ("family", "main-font"),
            ("size", "presentation-font-size-root"),
            ("line-height", "presentation-line-height"),
            // mermaid
            ("family", "mermaid-font-family"),
            ("weight", "mermaid-font-weight"),
        ],
        "headings" => &[
            // bootstrap
            ("family", "headings-font-family"),
            ("line-height", "headings-line-height"),
            ("weight", "headings-font-weight"),
            ("weight", "h1h2h3-font-weight"),
            ("color", "headings-color"),
            ("style", "headings-font-style"),
            // revealjs
            ("family", "presentation-heading-font"),
            ("line-height", "presentation-heading-line-height"),
            ("weight", "presentation-heading-font-weight"),
            ("color", "presentation-heading-color"),
        ],
        "link" => &[
            ("color", "link-color"),
            ("background-color", "link-color-bg"),
            ("weight", "link-weight"),
            ("decoration", "link-decoration"),
        ],
        "monospace" => &[
            ("family", "font-family-monospace"),
            ("size", "code-font-size"),
            ("color", "code-color"),
            ("color", "pre-color"),
            ("weight", "font-weight-monospace"),
            ("size", "code-block-font-size"),
            ("color", "code-block-color"),
            ("background-color", "code-bg"),
            ("background-color", "code-block-bg"),
        ],
        "monospace-block" => &[
            ("family", "font-family-monospace-block"),
            ("line-height", "pre-line-height"),
            ("color", "pre-color"),
            ("background-color", "pre-bg"),
            ("size", "code-block-font-size"),
            ("weight", "font-weight-monospace-block"),
            ("line-height", "code-block-line-height"),
            ("color", "code-block-color"),
            ("background-color", "code-block-bg"),
        ],
        "monospace-inline" => &[
            ("family", "font-family-monospace-inline"),
            ("color", "code-color"),
            ("background-color", "code-bg"),
            ("size", "code-inline-font-size"),
            ("weight", "font-weight-monospace-inline"),
        ],
        _ => &[],
    }
}

// ── font @import builders ───────────────────────────────────────────

fn google_font_import_string(font: &BrandFontGoogle) -> String {
    let family_url = font.family.replace(' ', "+");
    let styles = enumerate_styles(font.style.as_ref());
    let weights = enumerate_weights(font.weight.as_ref(), &[400, 700]);
    let display = font.display.as_deref().unwrap_or("swap");

    let mut style_string = String::new();
    let weights_string = if styles.iter().any(|s| s == "italic") {
        style_string.push_str("ital,");
        let normal_part = weights
            .iter()
            .map(|w| format!("0,{w}"))
            .collect::<Vec<_>>()
            .join(";");
        let italic_part = weights
            .iter()
            .map(|w| format!("1,{w}"))
            .collect::<Vec<_>>()
            .join(";");
        format!("{normal_part};{italic_part}")
    } else {
        weights
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(";")
    };

    format!(
        "@import url('https://fonts.googleapis.com/css2?family={family_url}:{style_string}wght@{weights_string}&display={display}');"
    )
}

fn bunny_font_import_string(font: &BrandFontGoogle) -> String {
    let family_url = font.family.replace(' ', "-");
    let styles = enumerate_styles(font.style.as_ref());
    let weights = enumerate_weights(font.weight.as_ref(), &[400, 700]);
    let display = font.display.as_deref().unwrap_or("swap");

    let weights_string = if styles.iter().any(|s| s == "italic") {
        let italic = weights
            .iter()
            .map(|w| format!("{w}i"))
            .collect::<Vec<_>>()
            .join(",");
        let normal = weights
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!("{italic},{normal}")
    } else {
        weights
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };

    format!(
        "@import url('https://fonts.bunny.net/css?family={family_url}:{weights_string}&display={display}');"
    )
}

fn enumerate_styles(style: Option<&BrandFontStyle>) -> Vec<String> {
    match style {
        None => vec!["normal".to_string(), "italic".to_string()],
        Some(BrandFontStyle::One(s)) => vec![s.clone()],
        Some(BrandFontStyle::List(v)) => v.clone(),
    }
}

fn enumerate_weights(weight: Option<&BrandFontWeight>, default: &[u32]) -> Vec<u32> {
    match weight {
        None => default.to_vec(),
        Some(BrandFontWeight::Number(n)) => vec![*n],
        Some(BrandFontWeight::Name(s)) => vec![weight_name_to_number(s).unwrap_or(400)],
        Some(BrandFontWeight::List(items)) => items
            .iter()
            .map(|a| match a {
                BrandFontWeightAtom::Number(n) => *n,
                BrandFontWeightAtom::Name(s) => weight_name_to_number(s).unwrap_or(400),
            })
            .collect(),
    }
}

fn file_font_face_block(font: &BrandFontFile, font_path_prefix: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for entry in &font.files {
        let (path, weight, style) = match entry {
            BrandFontFileEntry::Path(p) => (p.clone(), None, None),
            BrandFontFileEntry::Explicit {
                path,
                weight,
                style,
            } => (path.clone(), weight.clone(), style.clone()),
        };

        let font_url = if is_external_url(&path) {
            path.clone()
        } else {
            join_url_path(font_path_prefix, &path)
        };

        let weight_str = weight
            .as_ref()
            .map_or_else(|| "normal".to_string(), font_weight_to_scss);
        let style_str = style
            .as_ref()
            .map_or_else(|| "normal".to_string(), font_style_to_scss);

        parts.push(format!(
            "@font-face {{\n    font-family: {family};\n    src: url('{font_url}');\n    font-weight: {weight_str};\n    font-style: {style_str};\n}}",
            family = quote_family_name(&font.family),
        ));
    }
    parts.join("\n")
}

fn is_external_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("//")
}

/// Join a prefix and a relative path with forward slashes (URLs use
/// `/`, not OS separators).
fn join_url_path(prefix: &Path, rel: &str) -> String {
    if prefix.as_os_str().is_empty() {
        return rel.to_string();
    }
    let mut combined = PathBuf::from(prefix);
    combined.push(rel);
    combined.to_string_lossy().replace('\\', "/")
}

// ── error mapping ───────────────────────────────────────────────────

fn brand_err(e: quarto_brand::BrandError) -> SassError {
    SassError::InvalidThemeConfig {
        message: e.to_string(),
        location: None,
    }
}
