//! Syntax highlighting for Typst output.
//!
//! Pandoc's Typst writer has three modes, selected with
//! `--syntax-highlighting=`: `idiomatic` (bare fenced blocks, left to
//! Typst's own highlighter), `none`, or a skylighting style, which emits
//! `#Skylighting(...)`/`#KeywordTok(...)` calls plus a
//! `highlighting-definitions` block defining those functions.
//!
//! Pandoc's own definitions block draws the code background with a bare
//! `block(fill: bgcolor, blocks)` — no width, inset or radius — and has
//! no notion of a brand's `monospace-block` background. So for skylighting
//! styles this module generates the definitions block itself, from the
//! vendored `.theme` palette, and hands it to pandoc as the
//! `highlighting-definitions` variable (pandoc's writer sets that field
//! with `defField`, so a value supplied from outside wins and is emitted
//! verbatim). Nothing patches pandoc's output after the fact.
//!
//! Selection mirrors Quarto 1: `syntax-highlighting:` (or its deprecated
//! alias `highlight-style:`) names a palette, `none`, `idiomatic`, or a
//! `.theme` file; with neither set the default palette is `arrow`. Typst
//! has no light/dark switching of its own, so bare adaptive names
//! (`arrow`, `github`, …) resolve to the variant matching the document's
//! `brand-mode`.

use std::ffi::OsString;
use std::path::Path;

use quarto_pandoc_types::ConfigValue;
use quarto_sass::highlight_theme::{DotTheme, TextStyle};

/// Q1's `kDefaultHighlightStyle` (`pandoc.ts`).
const DEFAULT_PALETTE: &str = "arrow";

/// Pandoc's token names, in the (alphabetical) order its Typst writer
/// emits the `<Name>Tok` functions.
const TOKENS: &[&str] = &[
    "Alert",
    "Annotation",
    "Attribute",
    "BaseN",
    "BuiltIn",
    "Char",
    "Comment",
    "CommentVar",
    "Constant",
    "ControlFlow",
    "DataType",
    "DecVal",
    "Documentation",
    "Error",
    "Extension",
    "Float",
    "Function",
    "Import",
    "Information",
    "Keyword",
    "Normal",
    "Operator",
    "Other",
    "Preprocessor",
    "RegionMarker",
    "SpecialChar",
    "SpecialString",
    "String",
    "Variable",
    "VerbatimString",
    "Warning",
];

/// Why a highlighting configuration could not be turned into pandoc args.
#[derive(Debug)]
pub struct HighlightError(pub String);

impl std::fmt::Display for HighlightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The pandoc arguments that implement the document's highlighting
/// choice for Typst.
///
/// `brand_mode` is the document's `brand-mode` (`"dark"` selects dark
/// palette variants). `block_background` is the brand's resolved
/// `monospace-block` background color, if any; it replaces the palette's
/// own code-block background.
pub fn typst_highlight_args(
    doc_dir: &Path,
    meta: &ConfigValue,
    brand_mode: Option<&str>,
    block_background: Option<&str>,
) -> Result<Vec<OsString>, HighlightError> {
    let dark = brand_mode == Some("dark");
    let declared = declared_style(meta, dark);
    let style = declared.as_deref().unwrap_or("default");

    match style {
        "none" | "idiomatic" => Ok(vec![flag(style)]),
        _ => {
            let theme_json = load_theme_json(doc_dir, style, dark)?;
            let theme: DotTheme = serde_json::from_str(&theme_json)
                .map_err(|e| HighlightError(format!("invalid highlight theme `{style}`: {e}")))?;
            let definitions = highlighting_definitions(&theme, block_background);
            Ok(vec![
                // Pandoc still has to tokenize. Its Typst writer treats
                // `default` as `idiomatic`, so name a real style; which one
                // is irrelevant because `highlighting-definitions` below
                // replaces its definitions block wholesale.
                flag("pygments"),
                OsString::from("-V"),
                OsString::from(format!("highlighting-definitions={definitions}")),
            ])
        }
    }
}

fn flag(value: &str) -> OsString {
    OsString::from(format!("--syntax-highlighting={value}"))
}

/// `syntax-highlighting:` wins over the deprecated `highlight-style:`;
/// either may be a scalar or a `{light:, dark:}` pair.
fn declared_style(meta: &ConfigValue, dark: bool) -> Option<String> {
    let value = meta
        .get("syntax-highlighting")
        .or_else(|| meta.get("highlight-style"))?;
    let scalar = if value.get("light").is_some() || value.get("dark").is_some() {
        let (first, second) = if dark {
            ("dark", "light")
        } else {
            ("light", "dark")
        };
        value.get(first).or_else(|| value.get(second))?
    } else {
        value
    };
    scalar.as_plain_text()
}

/// Read the palette JSON: a `.theme` path relative to the document, or a
/// vendored palette name (with adaptive names resolved for `dark`).
fn load_theme_json(doc_dir: &Path, style: &str, dark: bool) -> Result<String, HighlightError> {
    if style.ends_with(".theme") {
        let path = doc_dir.join(style);
        return std::fs::read_to_string(&path).map_err(|e| {
            HighlightError(format!(
                "cannot read highlight theme `{}`: {e}",
                path.display()
            ))
        });
    }
    let name = if style == "default" {
        DEFAULT_PALETTE
    } else {
        style
    };
    let resolved = quarto_sass::resolve_adaptive_highlight(name, dark);
    quarto_sass::resources::HIGHLIGHT_STYLES_RESOURCES
        .read_str(Path::new(&format!("{resolved}.theme")))
        .map(str::to_string)
        .ok_or_else(|| {
            HighlightError(format!(
                "unknown syntax-highlighting style `{style}`; use one of \
                 `none`, `idiomatic`, a bundled palette name, or the path of a `.theme` file"
            ))
        })
}

/// The `highlighting-definitions` text: `EndLine`, the `Skylighting`
/// block function, and one `<Name>Tok` function per pandoc token.
pub fn highlighting_definitions(theme: &DotTheme, block_background: Option<&str>) -> String {
    let background = block_background
        .filter(|c| is_hex_color(c))
        .map(str::to_string)
        .or_else(|| {
            theme
                .background_color
                .clone()
                .or_else(|| theme.editor_colors.get("BackgroundColor")?.clone())
        })
        .map_or_else(
            || "none".to_string(),
            |c| format!("rgb(\"{}\")", c.to_lowercase()),
        );
    let line_numbers = theme
        .line_number_color
        .clone()
        .or_else(|| theme.editor_colors.get("LineNumbers")?.clone());
    let number_text = match line_numbers {
        Some(c) => format!("text(fill: rgb(\"{}\"), [ #lnum ])", c.to_lowercase()),
        None => "text([ #lnum ])".to_string(),
    };

    let mut out = String::new();
    out.push_str("/* Function definitions for syntax highlighting generated by skylighting: */\n");
    out.push_str("#let EndLine() = raw(\"\\n\")\n");
    out.push_str("#let Skylighting(fill: none, number: false, start: 1, sourcelines) = {\n");
    out.push_str("   let blocks = []\n");
    out.push_str("   let lnum = start - 1\n");
    out.push_str(&format!("   let bgcolor = {background}\n"));
    out.push_str("   for ln in sourcelines {\n");
    out.push_str("     if number {\n");
    out.push_str("       lnum = lnum + 1\n");
    out.push_str(&format!(
        "       blocks = blocks + box(width: if start + sourcelines.len() > 999 {{ 30pt }} else {{ 24pt }}, {number_text})\n"
    ));
    out.push_str("     }\n");
    out.push_str("     blocks = blocks + ln + EndLine()\n");
    out.push_str("   }\n");
    out.push_str("   block(fill: bgcolor, width: 100%, inset: 8pt, radius: 2pt, blocks)\n");
    out.push_str("}\n");
    // A token with no color of its own (absent, or `text-color: null`)
    // takes the plain-text color, as in skylighting.
    let default_color = theme
        .text_styles
        .get("Normal")
        .and_then(|s| s.text_color.as_deref())
        .or(theme.text_color.as_deref());
    let unstyled = TextStyle::default();
    for token in TOKENS {
        // KDE theme files spell the `Other` token `Others`.
        let style = theme
            .text_styles
            .get(*token)
            .or_else(|| (*token == "Other").then(|| theme.text_styles.get("Others"))?)
            .unwrap_or(&unstyled);
        out.push_str(&format!(
            "#let {token}Tok(s) = {}\n",
            token_body(style, default_color)
        ));
    }
    out
}

fn token_body(style: &TextStyle, default_color: Option<&str>) -> String {
    let mut args: Vec<String> = Vec::new();
    if style.bold {
        args.push("weight: \"bold\"".to_string());
    }
    if style.italic {
        args.push("style: \"italic\"".to_string());
    }
    if let Some(color) = style.text_color.as_deref().or(default_color) {
        args.push(format!("fill: rgb(\"{}\")", color.to_lowercase()));
    }
    args.push("raw(s)".to_string());
    let text = format!("text({})", args.join(","));
    if style.underline {
        format!("underline({text})")
    } else {
        text
    }
}

fn is_hex_color(s: &str) -> bool {
    s.strip_prefix('#')
        .is_some_and(|h| matches!(h.len(), 3 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::SourceInfo;

    fn meta(entries: &[(&str, &str)]) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::for_test(),
                    value: ConfigValue::new_string(*v, SourceInfo::for_test()),
                })
                .collect(),
            SourceInfo::for_test(),
        )
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn definitions_arg(args: &[OsString]) -> String {
        let s = strings(args);
        let i = s.iter().position(|a| a == "-V").expect("-V present");
        s[i + 1]
            .strip_prefix("highlighting-definitions=")
            .expect("definitions variable")
            .to_string()
    }

    #[test]
    fn none_and_idiomatic_are_plain_flags() {
        for style in ["none", "idiomatic"] {
            let args = typst_highlight_args(
                Path::new("."),
                &meta(&[("syntax-highlighting", style)]),
                None,
                None,
            )
            .unwrap();
            assert_eq!(strings(&args), [format!("--syntax-highlighting={style}")]);
        }
    }

    #[test]
    fn unset_defaults_to_arrow_with_styled_block() {
        let args = typst_highlight_args(Path::new("."), &meta(&[]), None, None).unwrap();
        let defs = definitions_arg(&args);
        assert!(defs.contains("let bgcolor = rgb(\"#f1f3f5\")"), "{defs}");
        assert!(
            defs.contains("block(fill: bgcolor, width: 100%, inset: 8pt, radius: 2pt, blocks)")
        );
        assert!(defs.contains("#let EndLine()"));
        assert!(
            defs.contains(
                "#let KeywordTok(s) = text(weight: \"bold\",fill: rgb(\"#003b4f\"),raw(s))"
            ),
            "{defs}"
        );
    }

    #[test]
    fn highlight_style_is_an_alias_and_syntax_highlighting_wins() {
        let alias = typst_highlight_args(
            Path::new("."),
            &meta(&[("highlight-style", "tango")]),
            None,
            None,
        )
        .unwrap();
        assert!(definitions_arg(&alias).contains("#204a87"));

        let both = typst_highlight_args(
            Path::new("."),
            &meta(&[
                ("highlight-style", "tango"),
                ("syntax-highlighting", "none"),
            ]),
            None,
            None,
        )
        .unwrap();
        assert_eq!(strings(&both), ["--syntax-highlighting=none"]);
    }

    #[test]
    fn brand_background_overrides_palette_background() {
        let args = typst_highlight_args(Path::new("."), &meta(&[]), None, Some("#1e1e2e")).unwrap();
        assert!(definitions_arg(&args).contains("let bgcolor = rgb(\"#1e1e2e\")"));
    }

    #[test]
    fn non_hex_brand_background_is_ignored() {
        let args =
            typst_highlight_args(Path::new("."), &meta(&[]), None, Some("rebeccapurple")).unwrap();
        assert!(definitions_arg(&args).contains("let bgcolor = rgb(\"#f1f3f5\")"));
    }

    #[test]
    fn brand_mode_dark_selects_dark_variant_of_adaptive_name() {
        let light = typst_highlight_args(
            Path::new("."),
            &meta(&[("syntax-highlighting", "arrow")]),
            None,
            None,
        )
        .unwrap();
        let dark = typst_highlight_args(
            Path::new("."),
            &meta(&[("syntax-highlighting", "arrow")]),
            Some("dark"),
            None,
        )
        .unwrap();
        assert_ne!(definitions_arg(&light), definitions_arg(&dark));
    }

    #[test]
    fn unknown_style_is_an_error() {
        let err = typst_highlight_args(
            Path::new("."),
            &meta(&[("syntax-highlighting", "no-such-style")]),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.0.contains("no-such-style"), "{err}");
    }

    #[test]
    fn theme_file_is_read_relative_to_the_document() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("mine.theme"),
            r##"{"background-color":"#fafafa","line-number-color":"#123456",
                "text-styles":{"Keyword":{"text-color":"#008000","bold":true,"underline":true}}}"##,
        )
        .unwrap();
        let args = typst_highlight_args(
            dir.path(),
            &meta(&[("syntax-highlighting", "mine.theme")]),
            None,
            None,
        )
        .unwrap();
        let defs = definitions_arg(&args);
        assert!(defs.contains("let bgcolor = rgb(\"#fafafa\")"));
        assert!(defs.contains("text(fill: rgb(\"#123456\"), [ #lnum ])"));
        assert!(defs.contains(
            "#let KeywordTok(s) = underline(text(weight: \"bold\",fill: rgb(\"#008000\"),raw(s)))"
        ));
        // A token the theme doesn't style is still defined.
        assert!(defs.contains("#let NormalTok(s) = text(raw(s))"));
    }

    #[test]
    fn missing_theme_file_is_an_error() {
        let err = typst_highlight_args(
            Path::new("/nonexistent-dir"),
            &meta(&[("syntax-highlighting", "gone.theme")]),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.0.contains("gone.theme"), "{err}");
    }
}
