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

use quarto_brand::{
    Brand, BrandFont, BrandFontFile, BrandFontGoogle, BrandFontStyle, BrandFontWeight,
    BrandFontWeightAtom, published_font_name,
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
/// `source: file` fonts are referenced as `fonts/<basename>` — the
/// location the theme stage publishes them to, beside the compiled
/// theme CSS (see [`quarto_brand::published_font_name`]). The URL is
/// therefore independent of where the brand file or the document
/// lives, which is what lets one compiled theme serve every page of a
/// site (bd-ve916wr8).
///
/// Returns an empty vector if the brand has no color, typography, or
/// `defaults.bootstrap` content.
pub fn brand_to_layers(brand: &Brand) -> Result<Vec<SassLayer>, SassError> {
    let mut layers = Vec::new();

    // Semantic validation before any emission (bd-5fseopxy): an
    // invalid `weight:` is reported with its YAML path here even when
    // the caller skipped `Brand::validate`. Callers that hold the
    // brand's source (config.rs) validate earlier and attach a span;
    // this path is the location-less backstop.
    brand.validate().map_err(brand_err)?;

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
        && let Some(typography) = typography_layer(brand)?
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

fn typography_layer(brand: &Brand) -> Result<Option<SassLayer>, SassError> {
    if brand.typography.is_none() {
        return Ok(None);
    }

    // `uses` collects font imports + @font-face blocks (Q1 puts these
    // in the `uses` section, even though it's mis-named — see module
    // docstring).
    let mut import_lines: Vec<String> = Vec::new();
    let mut seen_imports: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (i, font) in brand.fonts().iter().enumerate() {
        let font_path = format!("typography.fonts[{i}]");
        let line = match font {
            BrandFont::Google(g) => google_font_import_string(g, &font_path)?,
            BrandFont::Bunny(b) => bunny_font_import_string(b, &font_path)?,
            BrandFont::File(f) => file_font_face_block(f, &font_path)?,
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
                "weight" => options
                    .weight
                    .as_ref()
                    .map(|w| {
                        font_weight_to_css(w, &format!("typography.{kind}.weight"), RangeOk::No)
                    })
                    .transpose()?,
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

/// Where a weight is being rendered, which decides whether a range is
/// representable there.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RangeOk {
    /// `@font-face` — a range renders as CSS `font-weight: N M`, the
    /// declaration for a variable font file's axis.
    FontFace,
    /// A typography slot (`$headings-font-weight` etc.) — one weight
    /// only; a range here would be invalid SCSS.
    No,
}

/// Render a slot or `@font-face` weight as a CSS value.
///
/// Unknown keywords are errors, never a silent fallback (Q1 parity;
/// bd-5fseopxy). `Brand::validate` reports the same conditions first,
/// with the same YAML path, so these `Err` arms are the backstop for a
/// caller that skipped validation — deliberately not `unreachable!`.
fn font_weight_to_css(
    w: &BrandFontWeight,
    path: &str,
    range_ok: RangeOk,
) -> Result<String, SassError> {
    match w {
        BrandFontWeight::Number(n) => Ok(n.to_string()),
        BrandFontWeight::Range(r) => match range_ok {
            RangeOk::FontFace => Ok(format!("{} {}", r.min, r.max)),
            RangeOk::No => Err(weight_err(
                path,
                r,
                "a typography slot takes a single weight, not a range",
            )),
        },
        BrandFontWeight::Name(s) => weight_atom_value(s, path).map(|n| n.to_string()),
        BrandFontWeight::List(items) => {
            let values = items
                .iter()
                .enumerate()
                .map(|(i, a)| match a {
                    BrandFontWeightAtom::Number(n) => Ok(n.to_string()),
                    BrandFontWeightAtom::Name(s) => {
                        weight_atom_value(s, &format!("{path}[{i}]")).map(|n| n.to_string())
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(values.join(", "))
        }
    }
}

/// A weight written as a string: a keyword from the shared table, or a
/// quoted number (`"400"`).
fn weight_atom_value(s: &str, path: &str) -> Result<u32, SassError> {
    s.parse::<u32>()
        .ok()
        .or_else(|| quarto_brand::weight_name_to_number(s))
        .ok_or_else(|| weight_err(path, s, "not a weight keyword or a number from 100 to 900"))
}

/// Location-less [`SassError::InvalidBrandFontWeight`] for the emission
/// backstop; the source-located form is built in `config.rs` where the
/// brand text is in scope.
fn weight_err(path: &str, value: impl std::fmt::Display, reason: &str) -> SassError {
    SassError::InvalidBrandFontWeight {
        path: path.to_string(),
        value: value.to_string(),
        reason: reason.to_string(),
        location: None,
        brand_file: None,
    }
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

/// Google Fonts CSS2 import. A weight range is passed through as the
/// `wght` axis span (`wght@400..700`, or `ital,wght@0,400..700;1,400..700`
/// with italics), which is how the API serves one variable font file.
fn google_font_import_string(font: &BrandFontGoogle, font_path: &str) -> Result<String, SassError> {
    let family_url = font.family.replace(' ', "+");
    let styles = enumerate_styles(font.style.as_ref());
    let weights = weight_spec(
        font.weight.as_ref(),
        &[400, 700],
        &format!("{font_path}.weight"),
    )?
    .google_axis_values();
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
        weights.join(";")
    };

    Ok(format!(
        "@import url('https://fonts.googleapis.com/css2?family={family_url}:{style_string}wght@{weights_string}&display={display}');"
    ))
}

/// Bunny Fonts import. Bunny has no range syntax — `inter:400..700`
/// silently serves weight 400 only (checked 2026-09-08) — so a range is
/// expanded to discrete weights (see [`WeightSpec::bunny_weights`]).
fn bunny_font_import_string(font: &BrandFontGoogle, font_path: &str) -> Result<String, SassError> {
    let family_url = font.family.replace(' ', "-");
    let styles = enumerate_styles(font.style.as_ref());
    let weights = weight_spec(
        font.weight.as_ref(),
        &[400, 700],
        &format!("{font_path}.weight"),
    )?
    .bunny_weights();
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

    Ok(format!(
        "@import url('https://fonts.bunny.net/css?family={family_url}:{weights_string}&display={display}');"
    ))
}

fn enumerate_styles(style: Option<&BrandFontStyle>) -> Vec<String> {
    match style {
        None => vec!["normal".to_string(), "italic".to_string()],
        Some(BrandFontStyle::One(s)) => vec![s.clone()],
        Some(BrandFontStyle::List(v)) => v.clone(),
    }
}

/// The weights a google/bunny font entry asks for.
enum WeightSpec {
    /// Individual weights (`weight: 400`, `[400, bold]`, or the
    /// `[400, 700]` default).
    Discrete(Vec<u32>),
    /// A variable-font axis span (`weight: 400..700`).
    Range(u32, u32),
}

impl WeightSpec {
    /// Values for Google's `wght` axis: each discrete weight, or the
    /// single `N..M` span.
    fn google_axis_values(&self) -> Vec<String> {
        match self {
            WeightSpec::Discrete(v) => v.iter().map(u32::to_string).collect(),
            WeightSpec::Range(min, max) => vec![format!("{min}..{max}")],
        }
    }

    /// Discrete weights for Bunny, which has no range syntax: a range
    /// becomes both ends plus every multiple of 100 strictly between
    /// them (`400..700` → 400, 500, 600, 700; `450..620` → 450, 500,
    /// 600, 620).
    fn bunny_weights(&self) -> Vec<u32> {
        match self {
            WeightSpec::Discrete(v) => v.clone(),
            WeightSpec::Range(min, max) => {
                let mut out = vec![*min];
                let mut w = (min / 100 + 1) * 100;
                while w < *max {
                    out.push(w);
                    w += 100;
                }
                if max != min {
                    out.push(*max);
                }
                out
            }
        }
    }
}

fn weight_spec(
    weight: Option<&BrandFontWeight>,
    default: &[u32],
    path: &str,
) -> Result<WeightSpec, SassError> {
    Ok(match weight {
        None => WeightSpec::Discrete(default.to_vec()),
        Some(BrandFontWeight::Number(n)) => WeightSpec::Discrete(vec![*n]),
        Some(BrandFontWeight::Range(r)) => WeightSpec::Range(r.min, r.max),
        Some(BrandFontWeight::Name(s)) => WeightSpec::Discrete(vec![weight_atom_value(s, path)?]),
        Some(BrandFontWeight::List(items)) => WeightSpec::Discrete(
            items
                .iter()
                .enumerate()
                .map(|(i, a)| match a {
                    BrandFontWeightAtom::Number(n) => Ok(*n),
                    BrandFontWeightAtom::Name(s) => weight_atom_value(s, &format!("{path}[{i}]")),
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    })
}

/// One `@font-face` block per file. A per-file weight range renders as
/// CSS `font-weight: N M`, the declaration for a variable font's axis.
fn file_font_face_block(font: &BrandFontFile, font_path: &str) -> Result<String, SassError> {
    let mut parts: Vec<String> = Vec::new();
    for (j, entry) in font.files.iter().enumerate() {
        let path = entry.path();

        // A local file is published beside the theme CSS as
        // `fonts/<basename>` (the theme stage stores the bytes there);
        // an external URL is served by whoever hosts it. The URL is
        // the same for every page of a project, so the compiled theme
        // is document-independent.
        let src = match published_font_name(path) {
            Some(name) => match font_format_hint(&name) {
                Some(format) => format!("url('fonts/{name}') format('{format}')"),
                None => format!("url('fonts/{name}')"),
            },
            None => format!("url('{path}')"),
        };

        let weight_str = match entry.weight() {
            Some(w) => font_weight_to_css(
                w,
                &format!("{font_path}.files[{j}].weight"),
                RangeOk::FontFace,
            )?,
            None => "normal".to_string(),
        };
        let style_str = entry
            .style()
            .map_or_else(|| "normal".to_string(), font_style_to_scss);

        parts.push(format!(
            "@font-face {{\n    font-family: {family};\n    src: {src};\n    font-weight: {weight_str};\n    font-style: {style_str};\n}}",
            family = quote_family_name(&font.family),
        ));
    }
    Ok(parts.join("\n"))
}

/// The CSS `format()` hint for a font file, from its extension.
///
/// Browsers use the hint to skip downloads they cannot decode. Unknown
/// extensions get no hint rather than a guess.
fn font_format_hint(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    match ext.as_str() {
        "woff2" => Some("woff2"),
        "woff" => Some("woff"),
        "ttf" => Some("truetype"),
        "otf" => Some("opentype"),
        _ => None,
    }
}

// ── error mapping ───────────────────────────────────────────────────

pub(crate) fn brand_err(e: quarto_brand::BrandError) -> SassError {
    match e {
        quarto_brand::BrandError::InvalidFontWeight {
            path,
            value,
            reason,
        } => SassError::InvalidBrandFontWeight {
            path: path.to_string(),
            value,
            reason,
            location: None,
            brand_file: None,
        },
        other => SassError::InvalidThemeConfig {
            message: other.to_string(),
            location: None,
        },
    }
}
