//! Runtime translator from Quarto 1 `.theme` highlight palettes to
//! Quarto 2 SCSS layers.
//!
//! Copyright (c) 2026 Posit, PBC
//!
//! Quarto 1 ships its syntax-highlight palette catalog as
//! KDE-syntax-highlighting `.theme` JSON files (vendored under
//! `resources/pandoc/highlight-styles/`, embedded via
//! [`crate::resources::HIGHLIGHT_STYLES_RESOURCES`]). Those files
//! color *Pandoc/skylighting tokens* (`Keyword`, `String`, `Comment`,
//! …); Quarto 2's tree-sitter highlighter emits its own, strictly
//! finer-grained capture vocabulary (`hl-keyword`,
//! `hl-function-builtin`, …). This module bridges the two at render
//! time — mirroring Quarto 1, which also translates `.theme` JSON to
//! CSS at render time rather than shipping pre-generated stylesheets.
//!
//! The bridge is one **canonical mapping table** shared by every
//! palette (a `.theme` file carries nothing finer than the ~30 Pandoc
//! tokens, so per-palette tables would have nothing extra to
//! express): each capture in the emitted cover set resolves to the
//! Pandoc token that supplies its color/style, via longest
//! dotted-prefix fallback (`keyword.conditional` has its own entry →
//! `ControlFlow`; `variable.member` falls back to `variable` →
//! `Variable`). Palette-specific refinements beyond Q1 parity can be
//! layered later as hand-written override SCSS after the translated
//! layer; see `claude-notes/plans/2026-08-18-highlight-theme-translator.md`.

use serde::Deserialize;

use crate::error::SassError;
use crate::types::SassLayer;

// ---------------------------------------------------------------------------
// `.theme` JSON data model
// ---------------------------------------------------------------------------

/// One token's style in a `.theme` file's `text-styles` map.
///
/// `selected-text-color` also appears in the files but has no HTML
/// meaning (it styles editor selections), so it is not modeled.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TextStyle {
    #[serde(rename = "text-color")]
    pub text_color: Option<String>,
    #[serde(rename = "background-color")]
    pub background_color: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
}

/// A parsed `.theme` file.
///
/// `custom-styles` (per-KDE-language overrides) is deliberately not
/// modeled: Quarto 1's HTML output ignores it too (its CSS generator
/// reads only `text-styles`).
///
/// The maps are lookup-only (we iterate our own static token order,
/// never the maps), so plain `serde_json`-backed maps are fine for
/// determinism.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DotTheme {
    /// Pandoc token name → style.
    #[serde(rename = "text-styles", default)]
    pub text_styles: std::collections::HashMap<String, TextStyle>,
    /// Editor canvas colors; only `BackgroundColor` is consumed.
    #[serde(rename = "editor-colors", default)]
    pub editor_colors: std::collections::HashMap<String, Option<String>>,
    /// Top-level canvas color (some palettes, e.g. `none.theme`, use
    /// these instead of `editor-colors`).
    #[serde(rename = "background-color")]
    pub background_color: Option<String>,
    /// Top-level default text color.
    #[serde(rename = "text-color")]
    pub text_color: Option<String>,
}

// ---------------------------------------------------------------------------
// Canonical capture → Pandoc-token mapping
// ---------------------------------------------------------------------------

/// Capture (dotted tree-sitter name, possibly a prefix) → Pandoc
/// token supplying its color/style. Resolution walks dotted prefixes
/// (`a.b.c` → `a.b` → `a`), so refinements inherit their base
/// family's token unless explicitly overridden here.
///
/// Rationale notes live next to the non-obvious entries; the broad
/// strokes follow the group structure the stage-1 hand translations
/// established in `highlight-{default,a11y-light,a11y-dark}.scss`
/// (bd-0pic6 phase B), refined where a *distinct* Pandoc token
/// matches a capture family more faithfully (e.g. `ControlFlow`,
/// `BuiltIn`, `Documentation` — colors Q1 palettes genuinely
/// distinguish).
const CAPTURE_TOKENS: &[(&str, &str)] = &[
    // Keywords. Word-operators (`and`, `not`) and storage/function
    // keywords read as keywords; conditionals/loops are skylighting's
    // ControlFlow; directives are preprocessor material.
    ("keyword", "Keyword"),
    ("keyword.control", "ControlFlow"),
    ("keyword.conditional", "ControlFlow"),
    ("keyword.repeat", "ControlFlow"),
    ("keyword.directive", "Preprocessor"),
    // Strings. Escapes are skylighting's SpecialChar; regexes and
    // other special strings are SpecialString; char literals are Char.
    ("string", "String"),
    ("string.escape", "SpecialChar"),
    ("string.regexp", "SpecialString"),
    ("string.special", "SpecialString"),
    ("character", "Char"),
    ("escape", "SpecialChar"),
    // Numbers / booleans / constants.
    ("number", "DecVal"),
    ("boolean", "Constant"),
    ("constant", "Constant"),
    ("constant.numeric", "DecVal"),
    ("constant.character", "Char"),
    // Comments.
    ("comment", "Comment"),
    ("comment.documentation", "Documentation"),
    // Functions. Builtins are skylighting's BuiltIn (matches Q1's
    // rendering of e.g. Python's `print`); constructors read as
    // functions.
    ("function", "Function"),
    ("function.builtin", "BuiltIn"),
    ("constructor", "Function"),
    // Types / namespaces / modules.
    ("type", "DataType"),
    ("namespace", "DataType"),
    ("module", "DataType"),
    // Variables / properties. `variable.builtin` (`self`, `this`) is
    // BuiltIn, matching skylighting; properties read as variables;
    // attributes/labels use skylighting's Attribute.
    ("variable", "Variable"),
    ("variable.builtin", "BuiltIn"),
    ("property", "Variable"),
    ("attribute", "Attribute"),
    ("label", "Attribute"),
    // Operators / punctuation. Punctuation is Normal (body text
    // color); interpolation delimiters (`${}`) are SpecialChar.
    ("operator", "Operator"),
    ("punctuation", "Normal"),
    ("punctuation.special", "SpecialChar"),
    // Tags / markup. Tags read as keywords (the stage-1 precedent);
    // headings/links take the function color (the stage-1 precedent
    // for both `default` and `a11y`); raw spans read as strings.
    ("tag", "Keyword"),
    ("markup.heading", "Function"),
    ("markup.link", "Function"),
    ("markup.raw", "String"),
    // Special / embedded / errors.
    ("special", "SpecialChar"),
    ("embedded", "SpecialChar"),
    ("error", "Error"),
];

/// Resolve a dotted capture name to its Pandoc token via
/// longest-prefix fallback. Returns `None` for capture families the
/// canonical table doesn't know (they stay uncolored, like today).
pub fn capture_token(capture: &str) -> Option<&'static str> {
    let mut candidate = capture;
    loop {
        if let Some((_, token)) = CAPTURE_TOKENS.iter().find(|(c, _)| *c == candidate) {
            return Some(token);
        }
        candidate = &candidate[..candidate.rfind('.')?];
    }
}

/// One emitted capture: the dotted name plus palette-independent
/// extra declarations (structural styling like bold headings that no
/// Pandoc token expresses).
struct CoverEntry {
    capture: &'static str,
    extra: &'static [&'static str],
}

const NO_EXTRA: &[&str] = &[];
const BOLD_700: &[&str] = &["font-weight: 700"];
const ITALIC: &[&str] = &["font-style: italic"];
const STRIKE: &[&str] = &["text-decoration: line-through"];
const UNDERLINE: &[&str] = &["text-decoration: underline"];
const WAVY: &[&str] = &["text-decoration: underline wavy"];

macro_rules! cover {
    ($name:literal) => {
        CoverEntry {
            capture: $name,
            extra: NO_EXTRA,
        }
    };
    ($name:literal, $extra:expr) => {
        CoverEntry {
            capture: $name,
            extra: $extra,
        }
    };
}

/// The capture cover set: every `hl-*` class the translated palette
/// emits rules for, in output order. Mirrors the cover set of the
/// hand-written `highlight-default.scss` (the standard tree-sitter
/// capture vocabulary as emitted by `capture_to_class` in pampa's
/// HTML writer: dots become hyphens, `hl-` prefix).
const COVER_SET: &[CoverEntry] = &[
    // Keywords
    cover!("keyword"),
    cover!("keyword.control"),
    cover!("keyword.operator"),
    cover!("keyword.directive"),
    cover!("keyword.function"),
    cover!("keyword.storage"),
    cover!("keyword.conditional"),
    cover!("keyword.repeat"),
    // Strings / characters / escapes
    cover!("string"),
    cover!("string.escape"),
    cover!("string.regexp"),
    cover!("string.special"),
    cover!("string.special.symbol"),
    cover!("character"),
    cover!("escape"),
    // Numbers / booleans / constants
    cover!("number"),
    cover!("boolean"),
    cover!("constant"),
    cover!("constant.builtin"),
    cover!("constant.character"),
    cover!("constant.numeric"),
    // Comments
    cover!("comment"),
    cover!("comment.line"),
    cover!("comment.block"),
    cover!("comment.documentation"),
    cover!("comment.unused"),
    // Functions
    cover!("function"),
    cover!("function.method"),
    cover!("function.builtin"),
    cover!("function.special"),
    cover!("function.macro"),
    cover!("constructor"),
    cover!("constructor.builtin"),
    // Types / namespaces / modules
    cover!("type"),
    cover!("type.builtin"),
    cover!("type.parameter"),
    cover!("type.enum"),
    cover!("namespace"),
    cover!("module"),
    // Variables / properties
    cover!("variable"),
    cover!("variable.builtin"),
    cover!("variable.parameter"),
    cover!("variable.other"),
    cover!("variable.mutable"),
    cover!("variable.member"),
    cover!("property"),
    cover!("property.builtin"),
    cover!("attribute"),
    cover!("label"),
    // Operators / punctuation
    cover!("operator"),
    cover!("operator.word"),
    cover!("punctuation"),
    cover!("punctuation.bracket"),
    cover!("punctuation.delimiter"),
    cover!("punctuation.special"),
    // Tags / markup
    cover!("tag"),
    cover!("markup.heading", BOLD_700),
    cover!("markup.bold", BOLD_700),
    cover!("markup.italic", ITALIC),
    cover!("markup.strikethrough", STRIKE),
    cover!("markup.link", UNDERLINE),
    cover!("markup.link.url", UNDERLINE),
    cover!("markup.raw"),
    // Special / embedded / errors
    cover!("special"),
    cover!("embedded"),
    cover!("error", WAVY),
];

// ---------------------------------------------------------------------------
// Translation
// ---------------------------------------------------------------------------

/// Parse a `.theme` JSON string and translate it into a q2 SCSS layer.
///
/// - **defaults band**: `$code-block-bg` / `$code-block-color` from
///   the palette's canvas colors (top-level `background-color` /
///   `text-color` first, then `editor-colors.BackgroundColor` /
///   `text-styles.Normal.text-color`), plus Q1's
///   `resolveTextHighlightingLayer` copy-button feedback
///   (`$btn-code-copy-color` from `Comment`,
///   `$btn-code-copy-color-active` from `Function`). All `!default`,
///   so user theme layers and document metadata still win.
/// - **rules band**: one grouped rule per (token, extras) bucket over
///   the capture cover set, in canonical order.
///
/// `name` is used in error messages only.
pub fn translate_dot_theme(json: &str, name: &str) -> Result<SassLayer, SassError> {
    let theme: DotTheme = serde_json::from_str(json).map_err(|e| SassError::CompilationFailed {
        message: format!("highlight palette `{name}`: invalid .theme JSON: {e}"),
    })?;

    // ── defaults band ──────────────────────────────────────────────
    let mut defaults = String::new();
    let mut push_default = |var: &str, value: Option<&str>| -> Result<(), SassError> {
        if let Some(value) = value {
            validate_css_value(value, var, name)?;
            defaults.push_str(&format!("${var}: {value} !default;\n"));
        }
        Ok(())
    };

    let token_color = |token: &str| -> Option<&str> {
        theme
            .text_styles
            .get(token)
            .and_then(|s| s.text_color.as_deref())
    };

    let bg = theme
        .background_color
        .as_deref()
        .or_else(|| theme.editor_colors.get("BackgroundColor")?.as_deref());
    push_default("code-block-bg", bg)?;
    let fg = theme
        .text_color
        .as_deref()
        .or_else(|| token_color("Normal"));
    push_default("code-block-color", fg)?;
    // Q1's `resolveTextHighlightingLayer` copy-button feedback.
    push_default("btn-code-copy-color", token_color("Comment"))?;
    push_default("btn-code-copy-color-active", token_color("Function"))?;

    // ── rules band ─────────────────────────────────────────────────
    // Group cover-set captures by (resolved token, extras); a group's
    // declarations come from the token's style followed by the
    // palette-independent extras (later declarations win in CSS, so
    // an extra like `underline wavy` overrides a token's plain
    // `underline`). Cover-set order is preserved (first occurrence
    // defines group position), keeping output deterministic.
    struct Group {
        token: Option<&'static str>,
        extra: &'static [&'static str],
        selectors: Vec<String>,
    }
    let mut groups: Vec<Group> = Vec::new();
    for entry in COVER_SET {
        let token = capture_token(entry.capture);
        let selector = format!(".hl-{}", entry.capture.replace('.', "-"));
        match groups
            .iter_mut()
            .find(|g| g.token == token && g.extra == entry.extra)
        {
            Some(group) => group.selectors.push(selector),
            None => groups.push(Group {
                token,
                extra: entry.extra,
                selectors: vec![selector],
            }),
        }
    }

    let mut rules = String::new();
    // A palette with no token styles styles nothing — including the
    // palette-independent markup extras. This is what makes
    // `highlight-style: none` (an empty `text-styles`) a true no-op.
    if theme.text_styles.is_empty() {
        groups.clear();
    }
    for group in &groups {
        let mut decls: Vec<String> = Vec::new();
        if let Some(style) = group.token.and_then(|t| theme.text_styles.get(t)) {
            if let Some(color) = &style.text_color {
                validate_css_value(color, "text-color", name)?;
                decls.push(format!("color: {color}"));
            }
            if let Some(bg) = &style.background_color {
                validate_css_value(bg, "background-color", name)?;
                decls.push(format!("background-color: {bg}"));
            }
            if style.bold {
                decls.push("font-weight: bold".to_string());
            }
            if style.italic {
                decls.push("font-style: italic".to_string());
            }
            if style.underline {
                decls.push("text-decoration: underline".to_string());
            }
        }
        decls.extend(group.extra.iter().map(|d| d.to_string()));
        if decls.is_empty() {
            continue;
        }
        rules.push_str(&group.selectors.join(",\n"));
        rules.push_str(" {\n");
        for decl in &decls {
            rules.push_str(&format!("  {decl};\n"));
        }
        rules.push_str("}\n\n");
    }

    Ok(SassLayer {
        defaults,
        rules,
        ..Default::default()
    })
}

/// Whether a color-ish value is safe to splice into SCSS verbatim:
/// hex colors and CSS color keywords pass; anything carrying syntax
/// characters (`;`, `{`, quotes, …) is rejected so a `.theme` file or
/// a metadata value can never smuggle SCSS/CSS into the compiled
/// stylesheet. Shared with `derive_doc_scss_layer`'s
/// `code-block-bg` / `code-block-color` metadata forwarding in
/// quarto-core.
pub fn is_safe_css_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '#' || c == '-')
}

/// Validate a color value coming out of a `.theme` file before
/// splicing it into SCSS (see [`is_safe_css_value`]).
fn validate_css_value(value: &str, what: &str, name: &str) -> Result<(), SassError> {
    if is_safe_css_value(value) {
        Ok(())
    } else {
        Err(SassError::CompilationFailed {
            message: format!(
                "highlight palette `{name}`: {what} value {value:?} is not a valid color"
            ),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal palette fixture exercising every translation feature:
    /// distinct Keyword vs ControlFlow, flags (bold/italic/underline),
    /// per-token background, canvas colors, copy-button sources.
    const FIXTURE: &str = r##"{
        "editor-colors": { "BackgroundColor": "#fefef1" },
        "text-styles": {
            "Keyword":     { "text-color": "#aa0011", "bold": true },
            "ControlFlow": { "text-color": "#bb0022" },
            "String":      { "text-color": "#008001" },
            "Char":        { "text-color": "#008002" },
            "SpecialChar": { "text-color": "#008003" },
            "SpecialString": { "text-color": "#008004" },
            "Comment":     { "text-color": "#696961", "italic": true },
            "Documentation": { "text-color": "#696962" },
            "Function":    { "text-color": "#062871" },
            "BuiltIn":     { "text-color": "#062872" },
            "DataType":    { "text-color": "#7928a1" },
            "Variable":    { "text-color": "#a55a01" },
            "Attribute":   { "text-color": "#a55a02" },
            "Operator":    { "text-color": "#007691" },
            "DecVal":      { "text-color": "#792901" },
            "Constant":    { "text-color": "#792902" },
            "Error":       { "text-color": "#cc0001", "underline": true },
            "Alert":       { "text-color": "#cc0002", "background-color": "#ffeeee" },
            "Normal":      { "text-color": "#545451" }
        }
    }"##;

    fn layer() -> SassLayer {
        translate_dot_theme(FIXTURE, "fixture").expect("fixture translates")
    }

    /// One rule's declaration block, given a selector that must be in
    /// its selector list. Panics if the selector appears in no rule.
    fn decls_for(rules: &str, selector: &str) -> String {
        for block in rules.split('}') {
            let Some((selectors, decls)) = block.split_once('{') else {
                continue;
            };
            if selectors
                .split(',')
                .any(|s| s.trim().trim_start_matches('.') == selector.trim_start_matches('.'))
            {
                return decls.trim().to_string();
            }
        }
        panic!("no rule found for selector {selector} in:\n{rules}");
    }

    // -- capture_token resolution ------------------------------------------

    #[test]
    fn capture_token_direct_and_fallback() {
        assert_eq!(capture_token("keyword"), Some("Keyword"));
        // Explicit refinement wins over the base family.
        assert_eq!(capture_token("keyword.conditional"), Some("ControlFlow"));
        // Unmapped refinement falls back to its base family.
        assert_eq!(capture_token("variable.member"), Some("Variable"));
        assert_eq!(capture_token("keyword.function"), Some("Keyword"));
        // Multi-level fallback: a.b.c → a.b.
        assert_eq!(
            capture_token("string.special.symbol"),
            Some("SpecialString")
        );
        // Unknown family: uncolored.
        assert_eq!(capture_token("totally.unknown"), None);
    }

    // -- rules band ----------------------------------------------------------

    #[test]
    fn keyword_and_controlflow_get_distinct_colors() {
        let rules = layer().rules;
        let kw = decls_for(&rules, ".hl-keyword");
        assert!(kw.contains("color: #aa0011"), "keyword decls: {kw}");
        assert!(
            kw.contains("font-weight: bold"),
            "Keyword bold flag must translate: {kw}"
        );
        let cf = decls_for(&rules, ".hl-keyword-conditional");
        assert!(cf.contains("color: #bb0022"), "controlflow decls: {cf}");
        assert!(
            !cf.contains("font-weight"),
            "ControlFlow has no bold flag: {cf}"
        );
    }

    #[test]
    fn dotted_captures_flatten_to_hyphenated_classes() {
        let rules = layer().rules;
        // string.special.symbol → .hl-string-special-symbol
        let decls = decls_for(&rules, ".hl-string-special-symbol");
        assert!(decls.contains("color: #008004"), "got: {decls}");
    }

    #[test]
    fn comment_italic_flag_translates() {
        let rules = layer().rules;
        let decls = decls_for(&rules, ".hl-comment");
        assert!(decls.contains("color: #696961"), "got: {decls}");
        assert!(decls.contains("font-style: italic"), "got: {decls}");
        // comment.documentation refines to Documentation.
        let doc = decls_for(&rules, ".hl-comment-documentation");
        assert!(doc.contains("color: #696962"), "got: {doc}");
    }

    #[test]
    fn error_underline_flag_and_wavy_extra_coexist() {
        let rules = layer().rules;
        let decls = decls_for(&rules, ".hl-error");
        assert!(decls.contains("color: #cc0001"), "got: {decls}");
        // The palette-independent extra must win the band (one
        // text-decoration declaration, the wavy one, last).
        assert!(
            decls.contains("text-decoration: underline wavy"),
            "got: {decls}"
        );
    }

    #[test]
    fn markup_extras_emit_without_token_color() {
        let rules = layer().rules;
        let bold = decls_for(&rules, ".hl-markup-bold");
        assert!(bold.contains("font-weight: 700"), "got: {bold}");
        assert!(
            !bold.contains("color:"),
            "markup.bold takes no color: {bold}"
        );
        let heading = decls_for(&rules, ".hl-markup-heading");
        assert!(heading.contains("color: #062871"), "got: {heading}");
        assert!(heading.contains("font-weight: 700"), "got: {heading}");
        let link = decls_for(&rules, ".hl-markup-link");
        assert!(link.contains("text-decoration: underline"), "got: {link}");
    }

    #[test]
    fn builtin_refinements_get_builtin_color() {
        let rules = layer().rules;
        let f = decls_for(&rules, ".hl-function-builtin");
        assert!(f.contains("color: #062872"), "got: {f}");
        let v = decls_for(&rules, ".hl-variable-builtin");
        assert!(v.contains("color: #062872"), "got: {v}");
        // Non-refined siblings keep the family color.
        let m = decls_for(&rules, ".hl-function-method");
        assert!(m.contains("color: #062871"), "got: {m}");
    }

    #[test]
    fn unstyled_token_emits_no_rule() {
        // FIXTURE has no Preprocessor style → keyword.directive has a
        // token but no decls → no rule (and no empty rule).
        let rules = layer().rules;
        assert!(
            !rules.contains("hl-keyword-directive"),
            "unstyled token must not emit a selector: {rules}"
        );
        assert!(!rules.contains("{\n}"), "no empty rules: {rules}");
    }

    // -- defaults band ---------------------------------------------------------

    #[test]
    fn defaults_from_editor_colors_and_normal() {
        let defaults = layer().defaults;
        assert!(
            defaults.contains("$code-block-bg: #fefef1 !default;"),
            "got: {defaults}"
        );
        assert!(
            defaults.contains("$code-block-color: #545451 !default;"),
            "got: {defaults}"
        );
    }

    #[test]
    fn copy_button_colors_from_comment_and_function() {
        let defaults = layer().defaults;
        assert!(
            defaults.contains("$btn-code-copy-color: #696961 !default;"),
            "got: {defaults}"
        );
        assert!(
            defaults.contains("$btn-code-copy-color-active: #062871 !default;"),
            "got: {defaults}"
        );
    }

    #[test]
    fn top_level_canvas_colors_win() {
        let json = r##"{
            "background-color": "#111112",
            "text-color": "#eeeef1",
            "editor-colors": { "BackgroundColor": "#222221" },
            "text-styles": { "Normal": { "text-color": "#ccccc1" } }
        }"##;
        let layer = translate_dot_theme(json, "t").unwrap();
        assert!(
            layer.defaults.contains("$code-block-bg: #111112 !default;"),
            "got: {}",
            layer.defaults
        );
        assert!(
            layer
                .defaults
                .contains("$code-block-color: #eeeef1 !default;"),
            "got: {}",
            layer.defaults
        );
    }

    #[test]
    fn empty_theme_translates_to_empty_layer() {
        // none.theme's shape: null canvas colors, empty text-styles.
        let json = r##"{
            "text-color": null,
            "background-color": null,
            "text-styles": {}
        }"##;
        let layer = translate_dot_theme(json, "none").unwrap();
        assert!(layer.defaults.trim().is_empty(), "got: {}", layer.defaults);
        assert!(layer.rules.trim().is_empty(), "got: {}", layer.rules);
        assert!(layer.uses.is_empty());
        assert!(layer.functions.is_empty());
        assert!(layer.mixins.is_empty());
    }

    #[test]
    fn token_background_color_translates() {
        // Alert isn't in the cover set's token targets, so use a
        // fixture that puts a background on a covered token.
        let json = r##"{
            "text-styles": {
                "String": { "text-color": "#008001", "background-color": "#eeffee" }
            }
        }"##;
        let layer = translate_dot_theme(json, "t").unwrap();
        let decls = decls_for(&layer.rules, ".hl-string");
        assert!(decls.contains("color: #008001"), "got: {decls}");
        assert!(decls.contains("background-color: #eeffee"), "got: {decls}");
    }

    #[test]
    fn invalid_json_is_an_error() {
        assert!(translate_dot_theme("{ not json", "bad").is_err());
    }

    #[test]
    fn unsafe_color_values_are_rejected() {
        // A color value must not be able to smuggle SCSS/CSS syntax
        // into the compiled stylesheet.
        let json = r##"{
            "text-styles": {
                "Keyword": { "text-color": "#fff; } body { display: none" }
            }
        }"##;
        assert!(
            translate_dot_theme(json, "evil").is_err(),
            "non-color value must be rejected"
        );
    }

    // -- vendored catalog ------------------------------------------------------

    #[test]
    fn all_vendored_palettes_translate() {
        use crate::resources::HIGHLIGHT_STYLES_RESOURCES;
        let mut stems: Vec<String> = HIGHLIGHT_STYLES_RESOURCES
            .file_paths()
            .filter(|p| p.ends_with(".theme"))
            .map(|p| p.trim_end_matches(".theme").to_string())
            .collect();
        stems.sort();
        assert!(
            stems.len() >= 30,
            "expected the full Q1 catalog, found {} files",
            stems.len()
        );
        for stem in &stems {
            let json = HIGHLIGHT_STYLES_RESOURCES
                .read_str(std::path::Path::new(&format!("{stem}.theme")))
                .unwrap_or_else(|| panic!("{stem}.theme must be embedded UTF-8"));
            let layer = translate_dot_theme(json, stem)
                .unwrap_or_else(|e| panic!("{stem}.theme must translate: {e}"));
            if stem != "none" {
                assert!(
                    layer.rules.contains(".hl-keyword"),
                    "{stem}.theme should color keywords"
                );
            }
        }
    }

    #[test]
    fn github_light_translation_matches_source_colors() {
        use crate::resources::HIGHLIGHT_STYLES_RESOURCES;
        let json = HIGHLIGHT_STYLES_RESOURCES
            .read_str(std::path::Path::new("github-light.theme"))
            .expect("github-light.theme embedded");
        let layer = translate_dot_theme(json, "github-light").unwrap();
        // Keyword in github-light.theme is #d73a49.
        let kw = decls_for(&layer.rules, ".hl-keyword");
        assert!(kw.contains("color: #d73a49"), "got: {kw}");
        // Canvas: editor-colors.BackgroundColor #ffffff, Normal #24292e.
        assert!(
            layer.defaults.contains("$code-block-bg: #ffffff !default;"),
            "got: {}",
            layer.defaults
        );
        assert!(
            layer
                .defaults
                .contains("$code-block-color: #24292e !default;"),
            "got: {}",
            layer.defaults
        );
    }
}
