/*
 * title_banner.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Transform that derives title-block banner mode from metadata.
 */

//! Title-block banner normalization (bd-gx9cic8z P5, bd-364ol5lu).
//!
//! The Rust counterpart of Quarto 1's banner orchestration in
//! `format-html-title.ts` (`documentTitlePartial` +
//! `documentTitleIncludeInHeader` + the `processDocumentTitle` DOM
//! postprocessor — re-expressed without DOM surgery, per the
//! no-postprocessor rule):
//!
//! - **`rendered.title-block-banner`** — written (true) when
//!   `title-block-banner` is truthy. The `title-block` template
//!   partial branches on it (banner markup vs. default), and
//!   `FULL_HTML_TEMPLATE` uses it to emit the header above
//!   `#quarto-content` and add `quarto-banner-title-block` to
//!   `<main>` — the placements Q1 achieves by relocating DOM nodes.
//! - **Generated `<style>` include** (design decision Q5): explicit
//!   banner values produce an include-in-header style block pushed on
//!   [`RenderContext::includes`]. `title-block-banner: true` generates
//!   *no* block — colors come from the theme SCSS
//!   (`bannerBg()`/`bannerColor()` in `templates/title-block.scss`).
//!   A string value is an **image banner** when it names an existing
//!   file (absolute, or relative to the document's directory — Q1's
//!   `isBannerImage`), otherwise a **CSS background color**.
//!   `title-block-banner-color` sets the banner text color; the
//!   keywords `body` / `body-bg` are passed through as "use the theme
//!   default" (Q1's `titleColor()` returns undefined for both).
//! - **Image resource copy**: an image banner pushes a
//!   [`crate::render::ResourceCopyIntent`] so the file lands in the
//!   output tree at the URL the generated style references (the
//!   `ResourceCollectorTransform` pattern).
//!
//! Deliberately not ported (deviation documented in the epic plan):
//! `#quarto-header.quarto-banner` (Q2's navbar has no `#quarto-header`
//! wrapper, and the class's only Q1 consumer styles the website
//! secondary nav, which Q2 doesn't have). The `toc-left`
//! `banner-header-class` producer lives in
//! [`TocLocationTransform`](super::TocLocationTransform), not here —
//! it needs the normalized `toc-location`, which is a
//! Navigation-phase concern (bd-e2kpwy7n).
//!
//! Plan: `claude-notes/plans/2026-07-15-html-title-block-parity.md`
//! (Phase 5).

use std::path::{Path, PathBuf};

use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::{By, SourceInfo};

use std::sync::Arc;

use quarto_system_runtime::SystemRuntime;

use crate::Result;
use crate::render::{RenderContext, ResourceCopyIntent};
use crate::transform::{AstTransform, TransformPhase};

/// Transform that writes `rendered.title-block-banner` and the
/// explicit-banner style include into the render context.
///
/// Carries the [`SystemRuntime`] so the image-vs-color file-existence
/// probe works in both native (OS filesystem) and WASM (`/project/`
/// VFS) renders — a bare `Path::is_file()` cannot see VFS files (the
/// `ShortcodeResolveTransform` runtime-injection pattern).
pub struct TitleBannerTransform {
    runtime: Arc<dyn SystemRuntime>,
}

impl TitleBannerTransform {
    /// Create a new title-banner transform.
    pub fn new(runtime: Arc<dyn SystemRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for TitleBannerTransform {
    fn name(&self) -> &str {
        "title-banner"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Normalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Banner mode is an HTML title-block presentation feature. Q1
        // scopes it to format-html only — revealjs is also HTML-based
        // but has a title *slide*, not a title block, so it must not
        // receive the flag or a dead generated <style>. (`q2-preview`
        // maps to the Html identifier and is included.)
        if !matches!(ctx.format.identifier, crate::format::FormatIdentifier::Html) {
            return Ok(());
        }

        // `title-block-style: none` disables the banner entirely (Q1:
        // `documentTitlePartial` returns no partials for none/false,
        // so no banner markup or generated style). `plain` keeps the
        // banner (Q1 only drops the SCSS layer for plain).
        if crate::transforms::TitleBlockStyle::from_meta(&ast.meta)
            == crate::transforms::TitleBlockStyle::None
        {
            return Ok(());
        }

        let input_dir = ctx
            .document
            .input
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let is_file = |path: &Path| self.runtime.is_file(path).unwrap_or(false);
        let Some(banner) = classify_banner(&ast.meta, &input_dir, &is_file) else {
            return Ok(());
        };

        ast.meta.insert_path(
            &["rendered", "title-block-banner"],
            ConfigValue::new_bool(true, gen_si()),
        );

        let banner_color = explicit_banner_color(&ast.meta);
        if let Some(style) = banner_style_block(&banner, banner_color.as_deref()) {
            // Canonical include channel (the favicon-transform
            // pattern): reaches both the native template's
            // `$header-includes$` and the q2-preview head injector,
            // and lands after the theme CSS links in the head — the
            // Q5 source-order guarantee.
            super::website_favicon::append_to_rendered_header(&mut ast.meta, style);
        }

        if let Banner::Image(url) = &banner {
            let src = input_dir.join(url);
            let dest = ctx
                .resource_resolver
                .as_ref()
                .map(|r| r.page_dir().join(url));
            if let Some(dest) = dest {
                ctx.resource_copies.push(ResourceCopyIntent {
                    src,
                    dest,
                    origin: gen_si(),
                });
            }
        }

        Ok(())
    }
}

fn gen_si() -> SourceInfo {
    SourceInfo::generated(By::programmatic_config())
}

/// The three banner shapes a truthy `title-block-banner` can take.
#[derive(Debug, PartialEq, Eq)]
enum Banner {
    /// `title-block-banner: true` — theme-derived colors, no style block.
    ThemeDefault,
    /// A CSS background color/gradient string.
    Color(String),
    /// A document-relative (or absolute) image URL that exists on disk.
    Image(String),
}

/// Classify `title-block-banner`. `None` means "no banner"
/// (absent or `false`). `is_file` is the runtime-backed existence
/// probe (injected so this stays a pure function under test).
fn classify_banner(
    meta: &ConfigValue,
    input_dir: &Path,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<Banner> {
    let value = meta.get("title-block-banner")?;
    if let Some(b) = value.as_bool() {
        return b.then_some(Banner::ThemeDefault);
    }
    let s = value.as_plain_text()?;
    if s.is_empty() {
        return None;
    }
    Some(if is_banner_image(&s, input_dir, is_file) {
        Banner::Image(s)
    } else {
        Banner::Color(s)
    })
}

/// Q1's `isBannerImage`: a string banner is an image when it names an
/// existing file — absolute, or relative to the document's directory.
fn is_banner_image(banner: &str, input_dir: &Path, is_file: &dyn Fn(&Path) -> bool) -> bool {
    let path = Path::new(banner);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        input_dir.join(path)
    };
    is_file(&resolved)
}

/// The explicit banner text color, if any. The `body` / `body-bg`
/// keywords mean "theme default" and produce no inline color (Q1's
/// `titleColor()`).
fn explicit_banner_color(meta: &ConfigValue) -> Option<String> {
    let color = meta
        .get("title-block-banner-color")
        .and_then(|v| v.as_plain_text())?;
    match color.as_str() {
        "body" | "body-bg" => None,
        _ => Some(color),
    }
}

/// Build the include-in-header `<style>` block for an explicit banner,
/// or `None` when nothing needs generating (`title-block-banner: true`
/// with no explicit text color). Mirrors Q1's
/// `documentTitleIncludeInHeader`: a heading-color rule and a
/// container rule, both scoped under `.quarto-title-block` so the
/// generated block beats the theme SCSS by specificity (Q5).
fn banner_style_block(banner: &Banner, banner_color: Option<&str>) -> Option<String> {
    let mut heading_vars: Vec<String> = Vec::new();
    let mut container_vars: Vec<String> = Vec::new();

    if let Some(color) = banner_color {
        heading_vars.push(format!("color: {color};"));
        container_vars.push(format!("color: {color};"));
    }

    match banner {
        Banner::ThemeDefault => {}
        Banner::Image(url) => {
            container_vars.push(format!("background-image: url({url});"));
            container_vars.push("background-size: cover;".to_string());
        }
        Banner::Color(color) => {
            container_vars.push(format!("background: {color};"));
        }
    }

    if heading_vars.is_empty() && container_vars.is_empty() {
        return None;
    }

    let mut styles = String::from("<style>\n");
    if !heading_vars.is_empty() {
        styles.push_str(&format!(
            ".quarto-title-block .quarto-title-banner h1,\n\
             .quarto-title-block .quarto-title-banner h2,\n\
             .quarto-title-block .quarto-title-banner h3,\n\
             .quarto-title-block .quarto-title-banner h4,\n\
             .quarto-title-block .quarto-title-banner h5,\n\
             .quarto-title-block .quarto-title-banner h6 {{\n{}\n}}\n",
            heading_vars.join("\n")
        ));
    }
    if !container_vars.is_empty() {
        styles.push_str(&format!(
            ".quarto-title-block .quarto-title-banner {{\n{}\n}}\n",
            container_vars.join("\n")
        ));
    }
    styles.push_str("</style>");
    Some(styles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_pandoc_types::ConfigMapEntry;
    use quarto_source_map::{FileId, Location, Range};

    fn si() -> SourceInfo {
        SourceInfo::from_range(
            FileId(0),
            Range {
                start: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
                end: Location {
                    offset: 0,
                    row: 0,
                    column: 0,
                },
            },
        )
    }

    fn map(entries: Vec<(&str, ConfigValue)>) -> ConfigValue {
        ConfigValue::new_map(
            entries
                .into_iter()
                .map(|(k, v)| ConfigMapEntry {
                    key: k.to_string(),
                    key_source: si(),
                    value: v,
                })
                .collect(),
            si(),
        )
    }

    fn s(v: &str) -> ConfigValue {
        ConfigValue::new_string(v, si())
    }

    fn b(v: bool) -> ConfigValue {
        ConfigValue::new_bool(v, si())
    }

    /// std-fs-backed existence probe for native unit tests.
    fn fs_is_file(path: &Path) -> bool {
        path.is_file()
    }

    #[test]
    fn absent_and_false_are_no_banner() {
        let dir = Path::new(".");
        assert_eq!(classify_banner(&map(vec![]), dir, &fs_is_file), None);
        assert_eq!(
            classify_banner(
                &map(vec![("title-block-banner", b(false))]),
                dir,
                &fs_is_file
            ),
            None
        );
    }

    #[test]
    fn true_is_theme_default() {
        assert_eq!(
            classify_banner(
                &map(vec![("title-block-banner", b(true))]),
                Path::new("."),
                &fs_is_file
            ),
            Some(Banner::ThemeDefault)
        );
    }

    #[test]
    fn nonexistent_path_is_a_color() {
        // "#FFDDFF" names no file → CSS color.
        assert_eq!(
            classify_banner(
                &map(vec![("title-block-banner", s("#FFDDFF"))]),
                Path::new("/nonexistent"),
                &fs_is_file
            ),
            Some(Banner::Color("#FFDDFF".to_string()))
        );
    }

    #[test]
    fn existing_file_is_an_image() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(temp.path().join("banner.png"), b"png").unwrap();
        assert_eq!(
            classify_banner(
                &map(vec![("title-block-banner", s("banner.png"))]),
                temp.path(),
                &fs_is_file
            ),
            Some(Banner::Image("banner.png".to_string()))
        );
    }

    #[test]
    fn body_keywords_mean_theme_default_color() {
        for kw in ["body", "body-bg"] {
            let meta = map(vec![("title-block-banner-color", s(kw))]);
            assert_eq!(explicit_banner_color(&meta), None, "{kw}");
        }
        let meta = map(vec![("title-block-banner-color", s("#111111"))]);
        assert_eq!(explicit_banner_color(&meta), Some("#111111".to_string()));
    }

    #[test]
    fn theme_default_without_color_generates_no_style() {
        assert_eq!(banner_style_block(&Banner::ThemeDefault, None), None);
    }

    #[test]
    fn theme_default_with_color_generates_heading_and_container_color() {
        let style = banner_style_block(&Banner::ThemeDefault, Some("#111111")).unwrap();
        assert!(style.contains(".quarto-title-block .quarto-title-banner h1,"));
        assert!(style.contains("color: #111111;"));
        assert!(!style.contains("background"));
    }

    #[test]
    fn color_banner_generates_background() {
        let style = banner_style_block(&Banner::Color("#FFDDFF".to_string()), None).unwrap();
        assert!(style.contains(".quarto-title-block .quarto-title-banner {"));
        assert!(style.contains("background: #FFDDFF;"));
        assert!(!style.contains(" h1,"));
    }

    #[test]
    fn image_banner_generates_background_image_cover() {
        let style = banner_style_block(&Banner::Image("banner.png".to_string()), None).unwrap();
        assert!(style.contains("background-image: url(banner.png);"));
        assert!(style.contains("background-size: cover;"));
    }
}
