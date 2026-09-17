/*
 * transforms/hephaestus.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Render hephaestus plot documents (`.hep`) to SVG for HTML-family
//! output (`format: html`, `format: revealjs`).
//!
//! A [hephaestus](https://github.com/posit-dev/hephaestus) plot document
//! carries a plot's *configuration* — data, scales, geoms, theme, font
//! names — with nothing size-dependent baked in. Rendering it means
//! re-solving the layout at a size, which is what this transform does:
//! for every `Image` whose target is a `.hep` file it reads the bytes,
//! rebuilds the composition, draws it with hephaestus's renderer-free
//! SVG backend, stores the markup as a page-scoped artifact under
//! `figure-html/`, and points the image at the artifact's URL. The
//! writer then emits an ordinary `<img src="…svg">`.
//!
//! Design notes (bd-3qych45b, plan
//! `claude-notes/plans/2026-09-17-hephaestus-hep-integration.md`):
//!
//! - **SVG, not PNG.** Hephaestus rasterizes through wgpu, which needs
//!   a GPU adapter a headless render machine may not have, and GPU
//!   output is not byte-stable across machines. The SVG backend needs
//!   no GPU, keeps text as real `<text>` elements, and is
//!   deterministic. Other formats plug in here later (`latex` → the
//!   `pdf` backend, `typst` → SVG) keyed on `ctx.format`.
//! - **Bundled fonts.** A document names families; it does not carry
//!   them. Shaping with system fonts would make the same `.hep` render
//!   differently on macOS and Linux (different advances → different
//!   tick-label widths → a different layout). So the four Roboto faces
//!   hephaestus's own browser clients ship are embedded from
//!   `resources/hephaestus/fonts/` and registered once per process,
//!   with `sans-serif` mapped onto them — the same thing
//!   `examples/document_svg.rs` upstream does, so the markup produced
//!   here and the markup the wasm client draws in a preview agree.
//! - **Presentation transform**: [`TransformPhase::Finalization`],
//!   registered after `resource-collector` (which therefore still
//!   copies the `.hep` beside the page — a future live-reflow layer
//!   wants it there — and never looks for the generated SVG in the
//!   source tree) and before `responsive-image` (so the `<img>` gains
//!   `img-fluid` like any other body image).
//! - **Excluded from the q2-preview pipeline**
//!   (`Q2_PREVIEW_TRANSFORM_EXCLUDED`) and native-only at the cargo
//!   level: in `q2 preview` / hub-client the raw `.hep` `Image` reaches
//!   the React layer, where an `Image` wrapper over the npm
//!   `hephaestus-svg-wasm` client renders it live (phase 2 of the plan).
//! - **Fail soft.** A missing file, a document this build cannot read
//!   (wrong magic, incompatible format version, a custom geom) or a
//!   backend warning is a *warning* diagnostic, and the image is left
//!   untouched so the rest of the page still renders.
//!
//! Size: explicit `width` / `height` attributes (CSS pixels) win; else
//! the size the writer recorded in the document; else 7in × 5in at
//! 96 dpi, knitr's `fig-width` / `fig-height` defaults.
//!
//! Brand colors: when the document declares a brand (`brand:`), its
//! semantic colors recolor every plot through hephaestus's palette —
//! `background` → `paper`, `foreground` → `ink`, `primary` → `accent`.
//! Built-in hephaestus themes derive every chrome color from those
//! three anchors, so a dark-paper / light-ink brand inverts the whole
//! plot in one step. Only hex values can cross into the plot (a named
//! CSS color means nothing outside a browser); anything else warns
//! once per document (`Q-18-4`) and leaves that one anchor alone. The
//! light brand is used — dark mode is bd-myfwwmki.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use hephaestus::color::Color;
use hephaestus::document::{ReadContext, read_document};
use hephaestus::geometry::Size;
use hephaestus::svg::{SvgConfig, SvgScene, encode_svg};
use hephaestus::text::GenericFamilyKind;
use quarto_error_reporting::DiagnosticMessageBuilder;
use quarto_pandoc_types::ConfigValue;
use quarto_pandoc_types::inline::Image;
use quarto_pandoc_types::pandoc::Pandoc;
use quarto_source_map::SourceInfo;
use quarto_system_runtime::SystemRuntime;
use sha2::{Digest, Sha256};

use super::image_walk::for_each_image_mut;
use crate::Result;
use crate::artifact::{Artifact, ArtifactScope};
use crate::render::RenderContext;
use crate::transform::{AstTransform, TransformPhase};

/// File extension (without the dot) that marks a plot document.
pub const HEP_EXTENSION: &str = "hep";

/// CSS pixels per inch: the dpi the SVG backend is driven at, and what
/// the browser client always draws at. An SVG has no backing store, so
/// there is no device pixel ratio to fold in.
const CSS_DPI: f64 = 96.0;

/// 7in × 5in at 96 dpi — knitr's `fig-width` / `fig-height` defaults —
/// for a document with no size hint and an image with no attributes.
const DEFAULT_SIZE: (f64, f64) = (672.0, 480.0);

/// Artifact key prefix. Not `css:` / `js:`, so the template never emits
/// a `<link>` for a plot.
const ARTIFACT_KEY_PREFIX: &str = "hephaestus:";

/// Directory (under the page's `{stem}_files/`) rendered plots land in
/// — the same one engine-written figures use.
const ARTIFACT_DIR: &str = "figure-html";

/// The bundled faces, one file per (weight, style). Copied from
/// hephaestus's `crates/hephaestus-wasm/fonts/`; see
/// `resources/hephaestus/README.md`.
const BUNDLED_FONTS: [&[u8]; 4] = [
    include_bytes!("../../../../resources/hephaestus/fonts/roboto-regular.ttf"),
    include_bytes!("../../../../resources/hephaestus/fonts/roboto-bold.ttf"),
    include_bytes!("../../../../resources/hephaestus/fonts/roboto-italic.ttf"),
    include_bytes!("../../../../resources/hephaestus/fonts/roboto-bolditalic.ttf"),
];

/// See module docs.
pub struct HephaestusRenderTransform {
    runtime: Arc<dyn SystemRuntime>,
}

impl HephaestusRenderTransform {
    /// Create the transform. The runtime is how the `.hep` bytes are
    /// read — the native filesystem under `quarto render`.
    pub fn new(runtime: Arc<dyn SystemRuntime>) -> Self {
        Self { runtime }
    }
}

#[async_trait::async_trait(?Send)]
impl AstTransform for HephaestusRenderTransform {
    fn name(&self) -> &str {
        "hephaestus-render"
    }

    fn phase(&self) -> TransformPhase {
        TransformPhase::Finalization
    }

    async fn transform(&self, ast: &mut Pandoc, ctx: &mut RenderContext) -> Result<()> {
        // Self-gate: SVG in an <img> is an HTML-family answer. PDF
        // formats get their own branch here when they land.
        if !ctx.format.identifier.is_html_based() {
            return Ok(());
        }

        let mut ordinal = 0usize;
        let runtime = self.runtime.clone();
        let mut palette: Option<BrandPalette> = None;
        for_each_image_mut(&mut ast.blocks, &mut |img| {
            if !is_hep_target(&img.target.0) {
                return;
            }
            // Resolved on the first `.hep` only, so a brand-less or
            // plot-less document never touches the brand file.
            let palette =
                palette.get_or_insert_with(|| brand_palette(&ast.meta, runtime.as_ref(), ctx));
            ordinal += 1;
            render_image(img, ordinal, runtime.as_ref(), palette, ctx);
        });
        Ok(())
    }
}

/// True for a local (non-external, non-`data:`) URL ending in `.hep`,
/// ignoring any query string or fragment.
fn is_hep_target(url: &str) -> bool {
    if quarto_util::is_external_url(url) || url.starts_with("data:") {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(HEP_EXTENSION))
}

/// Resolve an image URL to the file it names: a leading `/` means the
/// project root (the path-resolution contract), anything else is
/// relative to the document.
fn resolve_source(url: &str, ctx: &RenderContext) -> PathBuf {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    match path.strip_prefix('/') {
        Some(stripped) => ctx.project.dir.join(stripped),
        None => ctx
            .document
            .input
            .parent()
            .unwrap_or(Path::new("."))
            .join(path),
    }
}

/// Where the reference sits in the source, for diagnostics: the URL's
/// own span when the parser recorded one, else the whole image.
fn origin(img: &Image) -> SourceInfo {
    img.target_source
        .url
        .clone()
        .unwrap_or_else(|| img.source_info.clone())
}

/// Read, render and rewrite one image. Every failure is reported as a
/// warning and leaves the image as authored.
fn render_image(
    img: &mut Image,
    ordinal: usize,
    runtime: &dyn SystemRuntime,
    palette: &BrandPalette,
    ctx: &mut RenderContext,
) {
    let url = img.target.0.clone();
    let source = resolve_source(&url, ctx);

    let bytes = match runtime.file_read(&source) {
        Ok(bytes) => bytes,
        Err(e) => {
            ctx.diagnostics.push(
                DiagnosticMessageBuilder::warning("Plot document not found")
                    .with_code("Q-18-1")
                    .with_location(origin(img))
                    .problem(format!(
                        "Cannot read the plot document `{}` (resolved to `{}`): {}",
                        url,
                        source.display(),
                        e
                    ))
                    .add_hint("Check the image path; it is resolved relative to the document, or to the project root when it starts with `/`")
                    .build(),
            );
            return;
        }
    };

    ensure_bundled_fonts();

    let doc = match read_document(&bytes, ReadContext::builtin()) {
        Ok(doc) => doc,
        Err(e) => {
            ctx.diagnostics.push(
                DiagnosticMessageBuilder::warning("Invalid plot document")
                    .with_code("Q-18-2")
                    .with_location(origin(img))
                    .problem(format!(
                        "`{}` is not a plot document this version of Quarto can read: {}",
                        url, e
                    ))
                    .add_hint(format!(
                        "Quarto reads hephaestus document format version {}; a document written by a newer library needs a newer Quarto, and a document using custom geoms or formatters cannot be rendered outside the program that wrote it",
                        hephaestus::document::FORMAT_VERSION_MAJOR
                    ))
                    .build(),
            );
            return;
        }
    };

    let (width, height) = choose_size(&img.attr.2, doc.hints.size);
    let mut composition = doc.composition;
    palette.apply(&mut composition);
    let id_prefix = format!("hep{ordinal}-");
    let config = SvgConfig::new()
        // The page-clearing rect follows the brand paper when there is
        // one; otherwise whatever the writer expected to draw over.
        .background(palette.paper.or(doc.hints.background))
        .id_prefix(id_prefix)
        .pick_ids(false);
    let size = Size::new(width, height);
    let mut scene = SvgScene::with_config(size, CSS_DPI, config);
    composition.render(&mut scene, size, CSS_DPI);

    if !scene.warnings().is_empty() {
        let list: Vec<String> = scene.warnings().iter().map(|w| format!("{w:?}")).collect();
        ctx.diagnostics.push(
            DiagnosticMessageBuilder::warning("Plot rendered with warnings")
                .with_code("Q-18-3")
                .with_location(origin(img))
                .problem(format!(
                    "`{}` uses features the SVG output cannot express exactly: {}",
                    url,
                    list.join("; ")
                ))
                .build(),
        );
    }

    let svg = encode_svg(&scene);
    let rel_path = artifact_path(&source, &bytes, width, height, palette);
    let html_url = match &ctx.resource_resolver {
        Some(resolver) => resolver.html_url_for(ArtifactScope::Page, &rel_path),
        None => rel_path.to_string_lossy().into_owned(),
    };
    ctx.artifacts.store(
        format!("{ARTIFACT_KEY_PREFIX}{}", rel_path.display()),
        Artifact::from_bytes(svg.into_bytes(), "image/svg+xml").with_path(rel_path),
    );
    img.target.0 = html_url;
}

/// The brand's semantic colors as hephaestus palette anchors. A `None`
/// slot leaves the document's own color in place.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
struct BrandPalette {
    paper: Option<Color>,
    ink: Option<Color>,
    accent: Option<Color>,
}

impl BrandPalette {
    /// Overwrite the composition theme's palette anchors that the brand
    /// sets. Per-plot theme overrides (`ThemePart`) still win, as they
    /// do over any composition theme.
    fn apply(&self, composition: &mut hephaestus::plot::PlotComposition) {
        if self.paper.is_none() && self.ink.is_none() && self.accent.is_none() {
            return;
        }
        composition.update_theme(|theme| {
            if let Some(paper) = self.paper {
                theme.palette.paper = paper;
            }
            if let Some(ink) = self.ink {
                theme.palette.ink = ink;
            }
            if let Some(accent) = self.accent {
                theme.palette.accent = accent;
            }
        });
    }
}

/// Brand semantic color → palette anchor.
const BRAND_SLOTS: [(&str, &str); 3] = [
    ("background", "paper"),
    ("foreground", "ink"),
    ("primary", "accent"),
];

/// Resolve the document's (light) brand, if any, into palette anchors.
/// Brand *loading* problems are not reported here — the theme stage
/// already reports them — but a color that cannot become an anchor is
/// (`Q-18-4`, once).
fn brand_palette(
    meta: &ConfigValue,
    runtime: &dyn SystemRuntime,
    ctx: &mut RenderContext,
) -> BrandPalette {
    let Ok(Some(brand)) = quarto_sass::resolve_brand(meta, runtime, &ctx.project.dir) else {
        return BrandPalette::default();
    };
    let Some(color) = brand.brand.color.as_ref() else {
        return BrandPalette::default();
    };

    let mut palette = BrandPalette::default();
    let mut rejected: Vec<String> = Vec::new();
    for (slot, anchor) in BRAND_SLOTS {
        // Only a slot the brand actually sets: `resolve_color` hands an
        // unset name back verbatim, which would read as a bad value.
        if color.named(slot).is_none() {
            continue;
        }
        let value = brand.brand.resolve_color_quiet(slot);
        match parse_hex_color(&value) {
            Some(c) => match anchor {
                "paper" => palette.paper = Some(c),
                "ink" => palette.ink = Some(c),
                _ => palette.accent = Some(c),
            },
            None => rejected.push(format!("`{slot}` = `{value}`")),
        }
    }

    if !rejected.is_empty() {
        ctx.diagnostics.push(
            DiagnosticMessageBuilder::warning("Brand color not applied to plot")
                .with_code("Q-18-4")
                .problem(format!(
                    "Only hex colors can recolor a plot document; the plot keeps its own color for {}",
                    rejected.join(", ")
                ))
                .add_hint("Write the color as `#rgb`, `#rrggbb` or `#rrggbbaa` in the brand file")
                .build(),
        );
    }
    palette
}

/// `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` → a color; anything else
/// (named CSS colors, `rgb()` functions) is `None`.
fn parse_hex_color(value: &str) -> Option<Color> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |i: usize| -> Option<u8> {
        match hex.len() {
            3 | 4 => {
                let d = u8::from_str_radix(&hex[i..i + 1], 16).ok()?;
                Some(d * 17)
            }
            6 | 8 => u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok(),
            _ => None,
        }
    };
    let (r, g, b) = (channel(0)?, channel(1)?, channel(2)?);
    let a = match hex.len() {
        4 | 8 => channel(3)?,
        _ => 255,
    };
    Some(hephaestus::color::rgba(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        f32::from(a) / 255.0,
    ))
}

/// `figure-html/<stem>-<hash>.svg`, where the hash covers everything
/// the markup depends on — the document bytes, the render size and the
/// brand palette — so the same plot at two sizes, or under two brands,
/// is two artifacts, and re-rendering an unchanged plot reuses the name.
fn artifact_path(
    source: &Path,
    bytes: &[u8],
    width: f64,
    height: f64,
    palette: &BrandPalette,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.update(width.to_le_bytes());
    hasher.update(height.to_le_bytes());
    for anchor in [palette.paper, palette.ink, palette.accent] {
        match anchor {
            Some(c) => hasher.update(c.to_rgba8().to_u8_array()),
            None => hasher.update([0xffu8; 5]),
        }
    }
    let digest = hex::encode(hasher.finalize());
    let stem = source
        .file_stem()
        .map_or_else(|| "plot".to_string(), |s| s.to_string_lossy().into_owned());
    Path::new(ARTIFACT_DIR).join(format!("{stem}-{}.svg", &digest[..12]))
}

/// Explicit `width` / `height` attributes (CSS px, a bare number or
/// `NNNpx`) win per axis; a missing or unparsable axis falls back to the
/// document's hint, then to [`DEFAULT_SIZE`]. Percentages and physical
/// units are not sizes the scene can be solved at; they fall through
/// and stay on the `<img>` for the browser.
fn choose_size(
    kvs: &hashlink::LinkedHashMap<String, String>,
    hint: Option<(f64, f64)>,
) -> (f64, f64) {
    let (hint_w, hint_h) = hint.unwrap_or(DEFAULT_SIZE);
    let width = kvs.get("width").and_then(|v| parse_px(v)).unwrap_or(hint_w);
    let height = kvs
        .get("height")
        .and_then(|v| parse_px(v))
        .unwrap_or(hint_h);
    (width, height)
}

fn parse_px(value: &str) -> Option<f64> {
    let trimmed = value.trim();
    let number = trimmed.strip_suffix("px").unwrap_or(trimmed).trim();
    number
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Register the bundled faces with hephaestus's process-global font
/// context and point `sans-serif` at them. Idempotent; the first
/// render pays it once.
fn ensure_bundled_fonts() {
    static REGISTERED: OnceLock<Vec<String>> = OnceLock::new();
    REGISTERED.get_or_init(|| {
        let mut families: Vec<String> = BUNDLED_FONTS
            .iter()
            .flat_map(|bytes| hephaestus::text::register_font_families(bytes.to_vec()))
            .collect();
        families.sort();
        families.dedup();
        hephaestus::text::set_generic_family(GenericFamilyKind::SansSerif, &families);
        families
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hep_targets_are_local_files_ending_in_hep() {
        assert!(is_hep_target("plot.hep"));
        assert!(is_hep_target("figs/plot.HEP"));
        assert!(is_hep_target("/figs/plot.hep?v=2"));
        assert!(is_hep_target("plot.hep#frag"));
        assert!(!is_hep_target("plot.png"));
        assert!(!is_hep_target("plot.hep.png"));
        assert!(!is_hep_target("https://example.com/plot.hep"));
        assert!(!is_hep_target("//cdn.example.com/plot.hep"));
        assert!(!is_hep_target("data:application/octet-stream;base64,AAAA"));
    }

    #[test]
    fn size_prefers_attributes_then_hint_then_default() {
        let mut kvs = hashlink::LinkedHashMap::new();
        assert_eq!(choose_size(&kvs, None), DEFAULT_SIZE);

        let hinted = Some((900.0, 420.0));
        assert_eq!(choose_size(&kvs, hinted), (900.0, 420.0));

        kvs.insert("width".to_string(), "300px".to_string());
        assert_eq!(choose_size(&kvs, hinted), (300.0, 420.0));
        kvs.insert("height".to_string(), "200".to_string());
        assert_eq!(choose_size(&kvs, hinted), (300.0, 200.0));

        // Not a pixel size: leave it to the browser, keep the hint.
        kvs.insert("width".to_string(), "50%".to_string());
        assert_eq!(choose_size(&kvs, hinted), (900.0, 200.0));
        kvs.insert("height".to_string(), "-4".to_string());
        assert_eq!(choose_size(&kvs, hinted), (900.0, 420.0));
    }

    #[test]
    fn artifact_path_is_content_and_size_addressed() {
        let none = BrandPalette::default();
        let branded = BrandPalette {
            paper: parse_hex_color("#101820"),
            ..BrandPalette::default()
        };
        let a = artifact_path(Path::new("figs/plot.hep"), b"abc", 900.0, 420.0, &none);
        let b = artifact_path(Path::new("elsewhere/plot.hep"), b"abc", 900.0, 420.0, &none);
        let c = artifact_path(Path::new("figs/plot.hep"), b"abc", 300.0, 420.0, &none);
        let d = artifact_path(Path::new("figs/plot.hep"), b"abd", 900.0, 420.0, &none);
        let e = artifact_path(Path::new("figs/plot.hep"), b"abc", 900.0, 420.0, &branded);
        assert_eq!(a, b, "the source directory is not part of the name");
        assert_ne!(a, c, "size participates");
        assert_ne!(a, d, "content participates");
        assert_ne!(a, e, "the brand palette participates");
        let name = a.to_string_lossy();
        assert!(name.starts_with("figure-html/plot-"), "{name}");
        assert!(name.ends_with(".svg"), "{name}");
    }

    #[test]
    fn hex_colors_parse_and_everything_else_does_not() {
        let hex = |s: &str| parse_hex_color(s).map(|c| c.to_rgba8().to_u8_array());
        assert_eq!(hex("#101820"), Some([0x10, 0x18, 0x20, 0xff]));
        assert_eq!(hex("  #F2F2F2 "), Some([0xf2, 0xf2, 0xf2, 0xff]));
        assert_eq!(hex("#abc"), Some([0xaa, 0xbb, 0xcc, 0xff]));
        assert_eq!(hex("#abcd"), Some([0xaa, 0xbb, 0xcc, 0xdd]));
        assert_eq!(hex("#ff6f6180"), Some([0xff, 0x6f, 0x61, 0x80]));
        assert_eq!(hex("rebeccapurple"), None);
        assert_eq!(hex("rgb(1, 2, 3)"), None);
        assert_eq!(hex("#12345"), None);
        assert_eq!(hex("#gg0000"), None);
        assert_eq!(hex(""), None);
    }

    #[test]
    fn bundled_fonts_register_roboto_for_sans_serif() {
        ensure_bundled_fonts();
        ensure_bundled_fonts(); // idempotent
        let mut families: Vec<String> = BUNDLED_FONTS
            .iter()
            .flat_map(|b| hephaestus::text::register_font_families(b.to_vec()))
            .collect();
        families.sort();
        families.dedup();
        assert_eq!(families, vec!["Roboto".to_string()]);
    }
}
