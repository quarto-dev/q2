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

use std::borrow::Cow;

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

/// Default reveal theme when `theme:` is absent. The reveal theme *resolver*
/// (`crate::revealjs::theme`) defaults to this; `white` aliases to it.
pub const DEFAULT_THEME: &str = "default";

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
    /// Asset content: `Borrowed` for the vendored static assets (reset, core
    /// reveal CSS/JS, quarto-reveal overrides); `Owned` for the theme slot when
    /// it carries a freshly-compiled Quarto reveal theme.
    pub content: Cow<'static, str>,
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
///
/// `compiled_theme_css` supplies the **theme slot** (`3-theme-<fingerprint>`):
/// `Some(css)` uses a freshly-compiled Quarto reveal theme (the normal path);
/// `None` falls back to the vendored stock reveal theme CSS — used if
/// reveal-theme compilation fails so a deck still renders.
///
/// The theme slot's key/filename carry a **content fingerprint** of the theme
/// CSS (like `format: html`'s `css:theme:<hash>`). This makes per-deck themes in
/// a website work correctly: identical themes dedup to one shared file; distinct
/// themes get distinct files and each deck links its own (collected per-document
/// from that deck's `StageContext.artifacts`). The `3-theme-` prefix keeps the
/// CSS cascade order (between `2-reveal` and `4-quarto-reveal`).
pub fn reveal_assets(compiled_theme_css: Option<&str>) -> Vec<RevealAsset> {
    let theme_content: Cow<'static, str> = match compiled_theme_css {
        Some(css) => Cow::Owned(css.to_string()),
        None => Cow::Borrowed(THEME_WHITE_CSS),
    };
    let fingerprint = crate::stage::stages::theme_fingerprint(&theme_content);
    vec![
        RevealAsset {
            key_suffix: "1-reset".to_string(),
            filename: "reset.css".to_string(),
            content: Cow::Borrowed(REVEAL_RESET_CSS),
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: "2-reveal".to_string(),
            filename: "reveal.css".to_string(),
            content: Cow::Borrowed(REVEAL_CSS),
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: format!("3-theme-{fingerprint}"),
            filename: format!("theme-{fingerprint}.css"),
            content: theme_content,
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: "4-quarto-reveal".to_string(),
            filename: "quarto-reveal.css".to_string(),
            content: Cow::Borrowed(QUARTO_REVEAL_CSS),
            content_type: "text/css",
            kind: RevealAssetKind::Css,
        },
        RevealAsset {
            key_suffix: "reveal".to_string(),
            filename: "reveal.js".to_string(),
            content: Cow::Borrowed(REVEAL_JS),
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
///
/// `compiled_theme_css` is the freshly-compiled Quarto reveal theme for the
/// theme slot; pass `None` to fall back to the vendored stock theme CSS (see
/// [`reveal_assets`]).
pub fn register_reveal_assets(artifacts: &mut ArtifactStore, compiled_theme_css: Option<&str>) {
    for asset in reveal_assets(compiled_theme_css) {
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
/// Maps Quarto/Pandoc option names to reveal.js config keys (camelCase) and
/// seeds Quarto 1's **opinionated default block** (`format-reveal.ts:343-361`),
/// which differs from reveal's own defaults in user-visible ways: no slide
/// transition, top-aligned slides (`center:false`), a 1050×700 deck with a 0.1
/// margin, linear navigation, edge controls, no controls tutorial. Each option
/// is overridable from front-matter (kebab-case keys, e.g. `controls-layout`).
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
    // No `as_f64` on ConfigValue; accept an int or a parseable numeric string.
    fn float_opt(meta: &ConfigValue, key: &str) -> Option<f64> {
        let v = meta.get(key)?;
        if let Some(i) = v.as_int() {
            return Some(i as f64);
        }
        v.as_plain_text().and_then(|s| s.trim().parse::<f64>().ok())
    }

    // Booleans — Quarto-1 opinionated defaults. `center: false` top-aligns
    // slides (reveal's own default is true); the title slide is re-centered via
    // a per-slide `.center` class (see `build_title_slide`).
    for (key, reveal_key, default) in [
        ("controls", "controls", true),
        ("progress", "progress", true),
        ("center", "center", false),
        ("hash", "hash", true),
        ("history", "history", true),
        ("controls-tutorial", "controlsTutorial", false),
        ("hash-one-based-index", "hashOneBasedIndex", false),
        ("fragment-in-url", "fragmentInURL", false),
        ("pdf-separate-fragments", "pdfSeparateFragments", false),
    ] {
        let value = bool_opt(meta, key).unwrap_or(default);
        map.insert(reveal_key.to_string(), serde_json::Value::Bool(value));
    }

    // Strings — Quarto-1 opinionated defaults (no transition, edge controls).
    let navigation_mode = str_opt(meta, "navigation-mode").unwrap_or_else(|| "linear".to_string());
    for (key, reveal_key, default) in [
        ("transition", "transition", "none"),
        ("background-transition", "backgroundTransition", "none"),
        ("controls-layout", "controlsLayout", "edges"),
    ] {
        let value = str_opt(meta, key).unwrap_or_else(|| default.to_string());
        map.insert(reveal_key.to_string(), serde_json::Value::String(value));
    }
    map.insert(
        "navigationMode".to_string(),
        serde_json::Value::String(navigation_mode.clone()),
    );
    if let Some(speed) = str_opt(meta, "transition-speed") {
        map.insert(
            "transitionSpeed".to_string(),
            serde_json::Value::String(speed),
        );
    }

    // Deck dimensions + margin — Quarto-1 opinionated defaults (1050×700, 0.1).
    // width/height accept an int or a percentage string (e.g. "100%").
    for (key, reveal_key, default) in [("width", "width", 1050), ("height", "height", 700)] {
        let value = int_opt(meta, key).map(serde_json::Value::from).or_else(|| {
            str_opt(meta, key)
                .filter(|s| s.ends_with('%'))
                .map(serde_json::Value::String)
        });
        map.insert(
            reveal_key.to_string(),
            value.unwrap_or_else(|| serde_json::Value::from(default)),
        );
    }
    map.insert(
        "margin".to_string(),
        serde_json::Value::from(float_opt(meta, "margin").unwrap_or(0.1)),
    );

    // slide-number: Quarto 1 rewrites `true` → "c/t" (linear navigation) or
    // "h.v" (vertical), and passes a format string through. Default: off.
    if let Some(v) = meta.get("slide-number") {
        if let Some(b) = v.as_bool() {
            if b {
                let vertical = matches!(navigation_mode.as_str(), "default" | "grid");
                let fmt = if vertical { "h.v" } else { "c/t" };
                map.insert(
                    "slideNumber".to_string(),
                    serde_json::Value::String(fmt.to_string()),
                );
            } else {
                map.insert("slideNumber".to_string(), serde_json::Value::Bool(false));
            }
        } else if let Some(s) = v.as_plain_text() {
            map.insert("slideNumber".to_string(), serde_json::Value::String(s));
        }
    }

    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .unwrap_or_else(|_| "{}".to_string())
}

/// Collect the deck-level footer + logo markup to place as **direct children of
/// `.reveal`** (outside `.slides`).
///
/// The markup is *pre-rendered* by [`RevealFooterLogoTransform`](super::RevealFooterLogoTransform)
/// into the `rendered.reveal.footer` / `rendered.reveal.logo` metadata slots
/// (footer routed through the format-agnostic `FooterGenerateTransform`; logo
/// from `logo:`). The scaffold only *places* it: footer/logo must be direct
/// children of `.reveal`, outside `.slides`, because reveal.js applies CSS
/// `transform` to `.slides`/`<section>`, under which a `position: fixed` element
/// resolves against the transformed ancestor rather than the viewport. Quarto 1
/// relocates the markup at runtime with its quarto-support reveal *plugin*;
/// Quarto 2 ships no plugin, so we place the single deck-level elements
/// statically here.
///
/// Returns `(markup, has_logo)`. `markup` is empty when neither slot is set;
/// `has_logo` drives the `.reveal.has-logo` class (slide-number repositioning).
fn footer_logo_html(meta: &ConfigValue) -> (String, bool) {
    let logo = meta
        .get_path(&["rendered", "reveal", "logo"])
        .and_then(|v| v.as_str());
    let footer = meta
        .get_path(&["rendered", "reveal", "footer"])
        .and_then(|v| v.as_str());

    // Logo first, then footer (matches Quarto 1's after-body order).
    let parts: Vec<&str> = [logo, footer].into_iter().flatten().collect();
    (parts.join("\n"), logo.is_some())
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

    // Deck-level footer/logo live OUTSIDE `.slides` (see `footer_logo_html`).
    let (footer_logo, has_logo) = footer_logo_html(meta);
    let reveal_class = if has_logo {
        "reveal has-logo"
    } else {
        "reveal"
    };
    // Emit on its own line only when present, so an empty deck stays clean.
    let footer_logo_block = if footer_logo.is_empty() {
        String::new()
    } else {
        format!("\n{footer_logo}")
    };

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
<div class="{reveal_class}">
<div class="slides">
{body}
</div>{footer_logo_block}
</div>
{scripts}
<script>
Reveal.initialize({config});
</script>
</body>
</html>
"#,
        title = escape_html(&title),
        reveal_class = reveal_class,
        links = links,
        body = body,
        footer_logo_block = footer_logo_block,
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
        // Quarto 1 defaults center:false — body slides top-align vertically
        // (reveal's own default is true; the title slide is re-centered via a
        // per-slide `.center` class, see build_title_slide).
        assert_eq!(v["center"], serde_json::json!(false));
        assert_eq!(v["hash"], serde_json::json!(true));
        // Quarto 1's opinionated block: no transition, 1050x700 / margin 0.1,
        // linear navigation, edge controls, no controls tutorial.
        assert_eq!(v["transition"], serde_json::json!("none"));
        assert_eq!(v["backgroundTransition"], serde_json::json!("none"));
        assert_eq!(v["width"], serde_json::json!(1050));
        assert_eq!(v["height"], serde_json::json!(700));
        assert_eq!(v["margin"], serde_json::json!(0.1));
        assert_eq!(v["navigationMode"], serde_json::json!("linear"));
        assert_eq!(v["controlsLayout"], serde_json::json!("edges"));
        assert_eq!(v["controlsTutorial"], serde_json::json!(false));
        assert_eq!(v["history"], serde_json::json!(true));
        assert_eq!(v["fragmentInURL"], serde_json::json!(false));
        assert_eq!(v["pdfSeparateFragments"], serde_json::json!(false));
    }

    #[test]
    fn config_maps_quarto_keys_to_reveal_keys() {
        let m = meta(vec![("transition", s("fade")), ("slide-number", b(true))]);
        let cfg = reveal_config_json(&m);
        let compact: String = cfg.chars().filter(|c| !c.is_whitespace()).collect();
        // Front-matter overrides the opinionated default.
        assert!(compact.contains("\"transition\":\"fade\""));
        // slide-number: true → Quarto's "c/t" format (linear navigation default).
        assert!(compact.contains("\"slideNumber\":\"c/t\""));
    }

    #[test]
    fn config_slide_number_vertical_navigation_uses_h_dot_v() {
        let m = meta(vec![
            ("slide-number", b(true)),
            ("navigation-mode", s("grid")),
        ]);
        let cfg = reveal_config_json(&m);
        let compact: String = cfg.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(compact.contains("\"slideNumber\":\"h.v\""));
        assert!(compact.contains("\"navigationMode\":\"grid\""));
    }

    #[test]
    fn config_front_matter_overrides_opinionated_defaults() {
        let m = meta(vec![
            ("transition", s("slide")),
            ("controls-layout", s("bottom-right")),
            ("history", b(false)),
        ]);
        let cfg = reveal_config_json(&m);
        let v: serde_json::Value = serde_json::from_str(&cfg).unwrap();
        assert_eq!(v["transition"], serde_json::json!("slide"));
        assert_eq!(v["controlsLayout"], serde_json::json!("bottom-right"));
        assert_eq!(v["history"], serde_json::json!(false));
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

    /// bd-ibqkf9ry: `q2 render` embeds the **vendored** `resources/revealjs/`
    /// copy (via `include_str!`, the constants below); `q2 preview` renders via
    /// `@revealjs/react`, which peer-depends on the **npm** `reveal.js` package
    /// (`node_modules/reveal.js/dist`). These MUST be the same bytes or the two
    /// pipelines drift on reveal version/CSS. This test compares the embedded
    /// constants to the npm dist and fails if the vendored copy is stale —
    /// re-sync `resources/revealjs/` from `node_modules/reveal.js/dist/` (see
    /// `resources/revealjs/README.md`). Skips when node_modules is absent
    /// (fresh checkout before `npm install`).
    #[test]
    fn vendored_reveal_assets_match_npm_package() {
        let npm_dist = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../node_modules/reveal.js/dist"
        );
        if !std::path::Path::new(npm_dist).exists() {
            eprintln!(
                "skipping reveal vendoring check: {npm_dist} absent (run `npm install` from repo root)"
            );
            return;
        }
        let check = |embedded: &str, rel: &str| {
            let path = format!("{npm_dist}/{rel}");
            let npm =
                std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
            assert_eq!(
                embedded, npm,
                "vendored resources/revealjs/{rel} has drifted from \
                 node_modules/reveal.js/dist/{rel} — re-sync the vendored copy \
                 (q2 render and q2 preview must use identical reveal assets)"
            );
        };
        check(REVEAL_RESET_CSS, "reset.css");
        check(REVEAL_CSS, "reveal.css");
        check(REVEAL_JS, "reveal.js");
        check(THEME_WHITE_CSS, "theme/white.css");
    }

    #[test]
    fn register_reveal_assets_stores_linkable_project_artifacts() {
        use crate::artifact::{ArtifactScope, ArtifactStore};
        use std::path::Path;

        let mut store = ArtifactStore::new();
        // None → stock vendored theme CSS in the theme slot (the fallback path).
        register_reveal_assets(&mut store, None);

        // The theme slot is keyed by a content fingerprint of the stock theme.
        let theme_fp = crate::stage::stages::theme_fingerprint(THEME_WHITE_CSS);
        let theme_key = format!("css:revealjs:3-theme-{theme_fp}");

        // CSS artifacts, keyed so sorted order == cascade order
        // (1-reset < 2-reveal < 3-theme-<fp> < 4-quarto-reveal).
        let mut css: Vec<&str> = store
            .get_by_prefix("css:revealjs:")
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        css.sort_unstable();
        assert_eq!(
            css,
            vec![
                "css:revealjs:1-reset",
                "css:revealjs:2-reveal",
                theme_key.as_str(),
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

        // theme artifact: fingerprinted path + the (fallback stock) theme bytes.
        let theme = store.get(&theme_key).unwrap();
        assert_eq!(
            theme.path.as_deref(),
            Some(Path::new(&*format!("revealjs/theme-{theme_fp}.css")))
        );
        assert_eq!(theme.as_str(), Some(THEME_WHITE_CSS));

        // reveal.js artifact.
        let core = store.get("js:revealjs:reveal").unwrap();
        assert_eq!(core.scope, ArtifactScope::Project);
        assert_eq!(core.path.as_deref(), Some(Path::new("revealjs/reveal.js")));
        assert_eq!(core.as_str(), Some(REVEAL_JS));
    }

    /// Distinct theme CSS → distinct fingerprinted keys (so per-deck themes in a
    /// website don't collide); identical theme CSS → identical key (dedups).
    #[test]
    fn register_reveal_assets_keys_theme_by_content_fingerprint() {
        use crate::artifact::ArtifactStore;

        let mut a = ArtifactStore::new();
        register_reveal_assets(&mut a, Some(".reveal{color:red}"));
        let mut b = ArtifactStore::new();
        register_reveal_assets(&mut b, Some(".reveal{color:blue}"));
        let mut b2 = ArtifactStore::new();
        register_reveal_assets(&mut b2, Some(".reveal{color:blue}"));

        let theme_key = |s: &ArtifactStore| {
            s.get_by_prefix("css:revealjs:3-theme-")
                .into_iter()
                .map(|(k, _)| k.to_string())
                .next()
                .unwrap()
        };
        assert_ne!(
            theme_key(&a),
            theme_key(&b),
            "different CSS → different keys"
        );
        assert_eq!(
            theme_key(&b),
            theme_key(&b2),
            "identical CSS → identical key (dedup)"
        );
    }

    /// A metadata map carrying a pre-rendered `rendered.reveal.<key>` slot, as
    /// `RevealFooterLogoTransform` would have populated it.
    fn meta_with_slot(slot: &str, html: &str) -> ConfigValue {
        let mut m = meta(vec![("title", s("T"))]);
        m.insert_path(&["rendered", "reveal", slot], s(html));
        m
    }

    /// The index where `.slides` closes — everything the deck-level footer/logo
    /// places must come *after* this (outside `.slides`) but before `.reveal`
    /// closes, so reveal's slide transforms don't break their fixed positioning.
    fn slides_close_idx(html: &str) -> usize {
        let slides_open = html.find(r#"<div class="slides">"#).expect("slides open");
        // the first `</div>` after the slides open closes `.slides`
        slides_open + html[slides_open..].find("</div>").expect("slides close")
    }

    #[test]
    fn footer_slot_placed_outside_slides() {
        let m = meta_with_slot(
            "footer",
            r#"<div class="footer footer-default">© 2026</div>"#,
        );
        let html = render_revealjs_document("<section></section>", &m, &sample_css(), &sample_js());
        let footer_at = html
            .find(r#"<div class="footer footer-default">"#)
            .expect("footer placed");
        assert!(
            footer_at > slides_close_idx(&html),
            "footer must be placed after `.slides` closes (outside it)"
        );
    }

    #[test]
    fn logo_slot_placed_outside_slides_and_marks_reveal() {
        let m = meta_with_slot("logo", r#"<img class="slide-logo" src="logo.png">"#);
        let html = render_revealjs_document("<section></section>", &m, &sample_css(), &sample_js());
        let logo_at = html.find(r#"class="slide-logo""#).expect("logo placed");
        assert!(
            logo_at > slides_close_idx(&html),
            "logo must be placed after `.slides` closes (outside it)"
        );
        // `.reveal` gains `has-logo` so slide-number repositioning kicks in.
        assert!(
            html.contains(r#"<div class="reveal has-logo">"#),
            "reveal element should carry `has-logo` when the logo slot is set"
        );
    }

    #[test]
    fn no_slots_emits_no_markup_and_no_has_logo() {
        let m = meta(vec![("title", s("T"))]);
        let html = render_revealjs_document("<section></section>", &m, &sample_css(), &sample_js());
        assert!(!html.contains("slide-logo"), "no logo markup expected");
        assert!(
            !html.contains("footer-default"),
            "no footer markup expected"
        );
        assert!(
            html.contains(r#"<div class="reveal">"#),
            "reveal element has no has-logo class without a logo slot"
        );
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
