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

use crate::artifact::{Artifact, ArtifactScope, ArtifactStore};

/// Vendored reveal.js 6 assets (see `resources/revealjs/README.md`).
const REVEAL_RESET_CSS: &str = include_str!("../../../../resources/revealjs/reset.css");
const REVEAL_CSS: &str = include_str!("../../../../resources/revealjs/reveal.css");
const REVEAL_JS: &str = include_str!("../../../../resources/revealjs/reveal.js");
const THEME_WHITE_CSS: &str = include_str!("../../../../resources/revealjs/theme/white.css");
/// Quarto's reveal layer (columns, …) — shared with the preview, which imports
/// the same file. Keep render/preview in sync by editing only the one file.
const QUARTO_REVEAL_CSS: &str = include_str!("../../../../resources/revealjs/quarto-reveal.css");

/// Default reveal theme when `theme:` is absent. Consumed by
/// `RevealAssetsStage` to pick the theme artifact.
pub const DEFAULT_THEME: &str = "white";

/// Resolve a requested theme name to the canonical name we actually ship.
/// Tier-1 ships only `white`; unknown themes fall back to it. Returning the
/// **resolved** name (not the requested one) keeps the artifact key/filename
/// stable so two decks that both fall back to `white` dedup to one copy.
fn resolve_theme_name(theme: &str) -> &'static str {
    match theme {
        "white" => "white",
        _ => "white",
    }
}

/// Theme CSS bytes for a *resolved* theme name (see [`resolve_theme_name`]).
fn theme_css(resolved: &str) -> &'static str {
    match resolved {
        "white" => THEME_WHITE_CSS,
        _ => THEME_WHITE_CSS,
    }
}

/// Kind of a vendored reveal asset (decides the `css:` / `js:` artifact key
/// prefix and the emitted tag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealAssetKind {
    Css,
    Js,
}

/// One vendored reveal asset to register as a **linkable** artifact
/// (bd-jij5gge2). The byte content is `include_str!`-embedded in the binary
/// (so `q2` stays a single binary); the artifact machinery extracts it to the
/// shared lib dir (`site_libs/revealjs/…` once per website) and the scaffold
/// links it — instead of inlining ~700 KB into every deck.
pub struct RevealAsset {
    /// Artifact-key suffix. Numeric prefix encodes the **CSS cascade order**
    /// (`collect_artifact_urls` emits in sorted-key order): reset → reveal →
    /// theme → quarto overrides. Theme keys also carry the resolved theme name
    /// so different themes get distinct (deduped-per-theme) artifacts.
    pub key_suffix: String,
    /// Output filename under the `revealjs/` lib dir.
    pub filename: String,
    /// Embedded byte content.
    pub content: &'static str,
    /// MIME type for the artifact.
    pub content_type: &'static str,
    pub kind: RevealAssetKind,
}

/// The ordered set of vendored assets a deck needs, for the configured theme.
///
/// Consumed by `RevealAssetsStage`, which registers each as a `Project`-scoped
/// `Artifact` (key `css:revealjs:<suffix>` / `js:revealjs:<suffix>`, path
/// `revealjs/<filename>`) so they flow through the same `site_libs` flush +
/// dedup as `format: html`'s bootstrap/theme deps.
pub fn reveal_assets(theme: &str) -> Vec<RevealAsset> {
    let resolved = resolve_theme_name(theme);
    vec![
        RevealAsset {
            key_suffix: "1-reset".to_string(),
            filename: "reset.css".to_string(),
            content: REVEAL_RESET_CSS,
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: "2-reveal".to_string(),
            filename: "reveal.css".to_string(),
            content: REVEAL_CSS,
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: format!("3-theme-{resolved}"),
            filename: format!("theme-{resolved}.css"),
            content: theme_css(resolved),
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: "4-quarto-reveal".to_string(),
            filename: "quarto-reveal.css".to_string(),
            content: QUARTO_REVEAL_CSS,
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: "reveal".to_string(),
            filename: "reveal.js".to_string(),
            content: REVEAL_JS,
            content_type: "application/javascript",
            kind: RevealAssetKind::Js,
        },
    ]
}

/// Register the deck's vendored assets as `Project`-scoped artifacts so the
/// existing `site_libs` flush + dedup machinery shares one copy across all
/// decks on a website (bd-jij5gge2). Keys are `css:revealjs:<suffix>` /
/// `js:revealjs:<suffix>` (the `apply_template` reveal branch collects exactly
/// these); paths are `revealjs/<filename>` under the resolved lib dir.
///
/// Called from `CompileThemeCssStage`'s reveal branch — the pipeline point that
/// already establishes a document's CSS-framework artifacts (and where the
/// Bootstrap theme path is skipped for reveal).
pub fn register_reveal_assets(artifacts: &mut ArtifactStore, theme: &str) {
    for asset in reveal_assets(theme) {
        let prefix = match asset.kind {
            RevealAssetKind::Css => "css",
            RevealAssetKind::Js => "js",
        };
        let key = format!("{prefix}:revealjs:{}", asset.key_suffix);
        let path = std::path::PathBuf::from(format!("revealjs/{}", asset.filename));
        artifacts.store(
            key,
            Artifact::from_string(asset.content, asset.content_type)
                .with_path(path)
                .with_scope(ArtifactScope::Project),
        );
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

/// Assemble the reveal.js HTML document, **linking** its CSS/JS assets.
///
/// * `body` — the rendered slide sections (the inner HTML of `.slides`).
/// * `meta` — merged, format-flattened document metadata.
/// * `css_urls` — context-resolved `<link>` hrefs for the deck's stylesheets,
///   already in cascade order (the caller collects them from the
///   `css:revealjs:*` artifacts via the resource resolver, so they are correct
///   for single-doc / website / preview).
/// * `js_urls` — context-resolved `<script src>` URLs (reveal.js core, in load
///   order; loaded before the per-document `Reveal.initialize`).
///
/// The vendored bytes are **not** inlined — they live once in the shared lib
/// dir and are referenced here (bd-jij5gge2). Only `Reveal.initialize(config)`,
/// which is per-document, stays inline.
pub fn render_revealjs_document(
    body: &str,
    meta: &ConfigValue,
    css_urls: &[String],
    js_urls: &[String],
) -> String {
    let title = meta
        .get("title")
        .and_then(|v| v.as_plain_text())
        .unwrap_or_default();
    let config = reveal_config_json(meta);

    let links = css_urls
        .iter()
        .map(|url| format!(r#"<link rel="stylesheet" href="{}">"#, attr_escape(url)))
        .collect::<Vec<_>>()
        .join("\n");
    let scripts = js_urls
        .iter()
        .map(|url| format!(r#"<script src="{}"></script>"#, attr_escape(url)))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
<title>{title}</title>
{links}
</head>
<body>
<div class="reveal">
<div class="slides">
{body}
</div>
</div>
{scripts}
<script>
Reveal.initialize({config});
</script>
</body>
</html>
"#,
        title = escape_html(&title),
        links = links,
        body = body,
        scripts = scripts,
        config = config,
    )
}

/// Escape a string for use inside a double-quoted HTML attribute value.
fn attr_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::{ConfigMapEntry, ConfigValue};
    use quarto_source_map::SourceInfo;

    fn meta(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: SourceInfo::default(),
                    value: v,
                })
                .collect(),
            SourceInfo::default(),
        )
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, SourceInfo::default())
    }
    fn b(v: bool) -> ConfigValue {
        ConfigValue::new_bool(v, SourceInfo::default())
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

    /// CSS/JS URLs as the stage + resolver would hand them to the
    /// assembler (already context-resolved, e.g. website `site_libs/…`).
    fn sample_css() -> Vec<String> {
        vec![
            "site_libs/revealjs/reset.css".to_string(),
            "site_libs/revealjs/reveal.css".to_string(),
            "site_libs/revealjs/theme-white.css".to_string(),
            "site_libs/revealjs/quarto-reveal.css".to_string(),
        ]
    }
    fn sample_js() -> Vec<String> {
        vec!["site_libs/revealjs/reveal.js".to_string()]
    }

    #[test]
    fn register_reveal_assets_stores_linkable_project_artifacts() {
        use crate::artifact::{ArtifactScope, ArtifactStore};
        use std::path::Path;

        let mut store = ArtifactStore::new();
        register_reveal_assets(&mut store, "white");

        // CSS artifacts, keyed so sorted order == cascade order.
        let mut css: Vec<&str> = store
            .get_by_prefix("css:revealjs:")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        css.sort();
        assert_eq!(
            css,
            vec![
                "css:revealjs:1-reset",
                "css:revealjs:2-reveal",
                "css:revealjs:3-theme-white",
                "css:revealjs:4-quarto-reveal",
            ]
        );

        // Exactly one JS artifact (reveal core).
        let js: Vec<&str> = store
            .get_by_prefix("js:revealjs:")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(js, vec!["js:revealjs:reveal"]);

        // reveal.css: Project-scoped, clean lib path, exact embedded bytes.
        let reveal = store.get("css:revealjs:2-reveal").unwrap();
        assert_eq!(reveal.scope, ArtifactScope::Project);
        assert_eq!(
            reveal.path.as_deref(),
            Some(Path::new("revealjs/reveal.css"))
        );
        assert_eq!(reveal.as_str(), Some(REVEAL_CSS));

        // theme artifact carries the resolved theme name + the theme bytes.
        let theme = store.get("css:revealjs:3-theme-white").unwrap();
        assert_eq!(
            theme.path.as_deref(),
            Some(Path::new("revealjs/theme-white.css"))
        );
        assert_eq!(theme.as_str(), Some(THEME_WHITE_CSS));

        // reveal.js artifact.
        let core = store.get("js:revealjs:reveal").unwrap();
        assert_eq!(core.scope, ArtifactScope::Project);
        assert_eq!(core.path.as_deref(), Some(Path::new("revealjs/reveal.js")));
        assert_eq!(core.as_str(), Some(REVEAL_JS));
    }

    /// An unknown theme falls back to `white` for both content AND
    /// key/filename, so two decks requesting unknown themes still dedup.
    #[test]
    fn register_reveal_assets_unknown_theme_falls_back_to_white() {
        use crate::artifact::ArtifactStore;
        let mut store = ArtifactStore::new();
        register_reveal_assets(&mut store, "no-such-theme");
        assert!(store.get("css:revealjs:3-theme-white").is_some());
        assert!(store.get("css:revealjs:3-theme-no-such-theme").is_none());
    }

    #[test]
    fn document_has_scaffold_and_init() {
        let m = meta(vec![("title", s("My Talk"))]);
        let html = render_revealjs_document(
            "<section><h2>S</h2></section>",
            &m,
            &sample_css(),
            &sample_js(),
        );
        assert!(html.contains("class=\"reveal\""));
        assert!(html.contains("class=\"slides\""));
        assert!(html.contains("Reveal.initialize"));
        assert!(html.contains("<title>My Talk</title>"));
    }

    /// bd-jij5gge2: the assembler must **link** vendored assets, not
    /// inline them — so a website with N decks shares one copy under
    /// `site_libs/revealjs/…` instead of duplicating ~700 KB per deck.
    #[test]
    fn links_assets_instead_of_inlining() {
        let m = meta(vec![("title", s("T"))]);
        let html = render_revealjs_document("<section></section>", &m, &sample_css(), &sample_js());

        // Linked, not inlined.
        assert!(
            html.contains(r#"<link rel="stylesheet" href="site_libs/revealjs/reveal.css">"#),
            "expected a <link> to reveal.css; got:\n{html}"
        );
        assert!(
            html.contains(r#"<script src="site_libs/revealjs/reveal.js"></script>"#),
            "expected a <script src> for reveal.js"
        );

        // The vendored core bytes must NOT appear inline anywhere.
        assert!(!html.contains(REVEAL_JS), "reveal.js must not be inlined");
        assert!(!html.contains(REVEAL_CSS), "reveal.css must not be inlined");
        assert!(
            !html.contains(QUARTO_REVEAL_CSS),
            "quarto-reveal.css must not be inlined"
        );
        assert!(
            !html.contains(THEME_WHITE_CSS),
            "theme css must not be inlined"
        );

        // Per-document config stays inline (it is not a shared asset).
        assert!(html.contains("Reveal.initialize"));

        // CSS cascade order is preserved from the input slice
        // (reset → reveal → theme → quarto overrides).
        let at = |needle: &str| html.find(needle).expect(needle);
        assert!(
            at(r#"href="site_libs/revealjs/reset.css""#)
                < at(r#"href="site_libs/revealjs/reveal.css""#)
        );
        assert!(
            at(r#"href="site_libs/revealjs/reveal.css""#)
                < at(r#"href="site_libs/revealjs/theme-white.css""#)
        );
        assert!(
            at(r#"href="site_libs/revealjs/theme-white.css""#)
                < at(r#"href="site_libs/revealjs/quarto-reveal.css""#)
        );

        // reveal.js <script src> must come before the inline initialize().
        assert!(at(r#"src="site_libs/revealjs/reveal.js""#) < at("Reveal.initialize"));
    }
}
