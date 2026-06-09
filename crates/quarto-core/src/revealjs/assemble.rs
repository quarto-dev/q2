/*
 * revealjs/assemble.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Assemble the final standalone reveal.js HTML document.
 */

//! Assemble a standalone reveal.js HTML document from a rendered slide body.
//!
//! The body (already a sequence of `<section>` slides produced by
//! [`RevealSlidesTransform`] + the HTML writer) is wrapped in the reveal
//! scaffold (`.reveal > .slides`). reveal.js core CSS/JS and the configured
//! theme are **inlined** (via `include_str!` of the vendored
//! `resources/revealjs/` copy) so the output is a single self-contained file
//! and `q2` stays a single binary. The `Reveal.initialize({…})` config is
//! built from the merged metadata (`format.revealjs.*`, already flattened to
//! top level by `MetadataMergeStage`).
//!
//! Tier-1 scope: linked (non-inlined) assets, additional themes, and plugins
//! are later phases of the revealjs epic.

use quarto_pandoc_types::ConfigValue;

/// Vendored reveal.js 6 assets (see `resources/revealjs/README.md`).
const REVEAL_RESET_CSS: &str = include_str!("../../../../resources/revealjs/reset.css");
const REVEAL_CSS: &str = include_str!("../../../../resources/revealjs/reveal.css");
const REVEAL_JS: &str = include_str!("../../../../resources/revealjs/reveal.js");
const THEME_WHITE_CSS: &str = include_str!("../../../../resources/revealjs/theme/white.css");
/// Quarto's reveal layer (columns, …) — shared with the preview, which imports
/// the same file. Keep render/preview in sync by editing only the one file.
const QUARTO_REVEAL_CSS: &str = include_str!("../../../../resources/revealjs/quarto-reveal.css");

const DEFAULT_THEME: &str = "white";

/// Resolve the theme CSS for the configured theme name. Tier-1 ships only the
/// `white` theme; unknown themes fall back to it (full theme set is a later
/// phase).
fn theme_css(theme: &str) -> &'static str {
    match theme {
        "white" => THEME_WHITE_CSS,
        _ => THEME_WHITE_CSS,
    }
}

/// Minimal HTML-text escape for the `<title>` element.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Build the `Reveal.initialize(...)` config object from merged metadata.
///
/// Maps Quarto/Pandoc option names to reveal.js config keys (camelCase). Only
/// Tier-1 options are wired; unknown keys are ignored. Defaults match Quarto 1
/// (controls/progress/center/hash on, `slide` transition).
fn reveal_config_json(meta: &ConfigValue) -> String {
    let mut map = serde_json::Map::new();

    fn bool_opt(meta: &ConfigValue, key: &str) -> Option<bool> {
        meta.get(key).and_then(|v| v.as_bool())
    }
    // YAML scalars in metadata are often parsed as `PandocInlines`, which
    // `as_str()` misses; `as_plain_text()` also extracts text from inlines.
    fn str_opt(meta: &ConfigValue, key: &str) -> Option<String> {
        meta.get(key).and_then(|v| v.as_plain_text())
    }
    fn int_opt(meta: &ConfigValue, key: &str) -> Option<i64> {
        meta.get(key).and_then(|v| v.as_int())
    }

    // Booleans with Quarto-1 defaults.
    for (key, reveal_key, default) in [
        ("controls", "controls", true),
        ("progress", "progress", true),
        ("center", "center", true),
        ("hash", "hash", true),
    ] {
        let value = bool_opt(meta, key).unwrap_or(default);
        map.insert(reveal_key.to_string(), serde_json::Value::Bool(value));
    }

    // Transition (string), default "slide".
    let transition = str_opt(meta, "transition").unwrap_or_else(|| "slide".to_string());
    map.insert(
        "transition".to_string(),
        serde_json::Value::String(transition),
    );
    if let Some(speed) = str_opt(meta, "transition-speed") {
        map.insert(
            "transitionSpeed".to_string(),
            serde_json::Value::String(speed),
        );
    }

    // slide-number: either a bool or a format string (e.g. "c/t").
    if let Some(v) = meta.get("slide-number") {
        if let Some(b) = v.as_bool() {
            map.insert("slideNumber".to_string(), serde_json::Value::Bool(b));
        } else if let Some(s) = v.as_plain_text() {
            map.insert("slideNumber".to_string(), serde_json::Value::String(s));
        }
    }

    // Explicit deck dimensions.
    if let Some(w) = int_opt(meta, "width") {
        map.insert("width".to_string(), serde_json::Value::from(w));
    }
    if let Some(h) = int_opt(meta, "height") {
        map.insert("height".to_string(), serde_json::Value::from(h));
    }

    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .unwrap_or_else(|_| "{}".to_string())
}

/// Assemble the standalone reveal.js HTML document.
///
/// * `body` — the rendered slide sections (the inner HTML of `.slides`).
/// * `meta` — merged, format-flattened document metadata.
pub fn render_revealjs_document(body: &str, meta: &ConfigValue) -> String {
    let title = meta
        .get("title")
        .and_then(|v| v.as_plain_text())
        .unwrap_or_default();
    let theme = meta
        .get("theme")
        .and_then(|v| v.as_plain_text())
        .unwrap_or_else(|| DEFAULT_THEME.to_string());
    let config = reveal_config_json(meta);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
<title>{title}</title>
<style>
{reset}
{reveal}
</style>
<style id="theme">
{theme_css}
</style>
<style id="quarto-reveal">
{quarto_css}
</style>
</head>
<body>
<div class="reveal">
<div class="slides">
{body}
</div>
</div>
<script>
{reveal_js}
</script>
<script>
Reveal.initialize({config});
</script>
</body>
</html>
"#,
        title = escape_html(&title),
        reset = REVEAL_RESET_CSS,
        reveal = REVEAL_CSS,
        theme_css = theme_css(&theme),
        quarto_css = QUARTO_REVEAL_CSS,
        body = body,
        reveal_js = REVEAL_JS,
        config = config,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::{By, SourceInfo};

    fn meta(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::generated(By::revealjs()),
                    value: v,
                })
                .collect(),
            SourceInfo::generated(By::revealjs()),
        )
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::generated(By::revealjs()))
    }
    fn b(v: bool) -> ConfigValue {
        ConfigValue::new_bool(v, SourceInfo::generated(By::revealjs()))
    }

    #[test]
    fn config_defaults_match_quarto1() {
        let m = meta(vec![("title", s("T"))]);
        let cfg = reveal_config_json(&m);
        let v: serde_json::Value = serde_json::from_str(&cfg).unwrap();
        assert_eq!(v["controls"], serde_json::json!(true));
        assert_eq!(v["progress"], serde_json::json!(true));
        assert_eq!(v["center"], serde_json::json!(true));
        assert_eq!(v["hash"], serde_json::json!(true));
        assert_eq!(v["transition"], serde_json::json!("slide"));
    }

    #[test]
    fn config_maps_quarto_keys_to_reveal_keys() {
        let m = meta(vec![("transition", s("fade")), ("slide-number", b(true))]);
        let cfg = reveal_config_json(&m);
        let compact: String = cfg.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(compact.contains("\"transition\":\"fade\""));
        assert!(compact.contains("\"slideNumber\":true"));
    }

    #[test]
    fn document_has_scaffold_and_init() {
        let m = meta(vec![("title", s("My Talk"))]);
        let html = render_revealjs_document("<section><h2>S</h2></section>", &m);
        assert!(html.contains("class=\"reveal\""));
        assert!(html.contains("class=\"slides\""));
        assert!(html.contains("Reveal.initialize"));
        assert!(html.contains("<title>My Talk</title>"));
    }
}
