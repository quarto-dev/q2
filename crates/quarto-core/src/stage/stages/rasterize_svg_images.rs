//! `RasterizeSvgImagesStage`: pandoc's docx and pptx writers turn an SVG image
//! into a PNG fallback with `rsvg-convert`, which pandoc.wasm does not have, so
//! an SVG in a browser docx/pptx download would show as alt text. Before the
//! request is built, every local `.svg` image the document names is rasterized
//! through the runtime (`SystemRuntime::rasterize_svg`: the browser's `<img>`
//! and `<canvas>`), mounted as `<doc_dir>/_raster/<hash>.png`, and the
//! `Image`'s `src` is rewritten to that absolute path. The original target stays
//! on the image as the `q2-raster-src` attribute. The extension decides for
//! pandoc, not the bytes, so the rewritten target must end in `.png`.
//!
//! Only for docx and pptx (epub and typst keep the SVG, as do html and the
//! preview), and only where the runtime can rasterize: native and node never
//! walk the document. Sits after [`PrefetchRemoteImagesStage`] so a remote SVG
//! fetched into `_remote/` is rasterized too, and just before `pandoc-prepare`,
//! which then mounts the PNG like any local image.
//!
//! Failure policy: an SVG that cannot be rasterized (undecodable, tainted
//! canvas, timeout, over a limit) stays an SVG, pandoc shows its alt text as
//! it does today, and a warning names the file. Never an error: the mount
//! limits would fail the whole download. A runtime that cannot rasterize at
//! all is not a failure and is silent.
//!
//! The runtime decides the pixel size, so Rust never parses SVG or its units.
//!
//! [`PrefetchRemoteImagesStage`]: super::PrefetchRemoteImagesStage

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::Inline;
use quarto_system_runtime::RuntimeError;
use sha2::{Digest, Sha256};

use crate::format::FormatIdentifier;
use crate::pandoc_request::{constants, normalize_request_path, resolve_image_target};
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

/// The attribute that keeps the SVG target an image was rasterized from.
pub const RASTER_SRC_ATTR: &str = "q2-raster-src";

/// The directory next to the document that holds the rasterized images.
pub const RASTER_DIR: &str = "_raster";

/// Longest side of a rasterized PNG, in pixels (a 6.5 in page at 300 dpi is
/// about 1950).
const MAX_SIDE: u32 = 2048;

#[derive(Default)]
pub struct RasterizeSvgImagesStage;

impl RasterizeSvgImagesStage {
    pub fn new() -> Self {
        Self
    }
}

/// An image target whose file name ends in `.svg`, whatever its case. A
/// target with a query or fragment (`a.svg?x=1`) is not a file and does not
/// match.
fn names_an_svg(target: &str) -> bool {
    resolve_image_target(Path::new(""), target, None).is_some_and(|path| {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"))
    })
}

/// `<hash>.png`: the first 16 hex digits of the SVG bytes' SHA-256.
fn mounted_name(svg: &[u8]) -> String {
    format!("{}.png", &hex::encode(Sha256::digest(svg))[..16])
}

fn warning(message: String) -> DiagnosticMessage {
    // The resource code pandoc's own warning for the same image would carry.
    DiagnosticMessageBuilder::warning(message)
        .with_code("Q-11-1")
        .build()
}

/// `path` as the project sees it: under `root` (both normalized).
fn is_under(path: &str, root: &str) -> bool {
    path == root || path.starts_with(&format!("{}/", root.trim_end_matches('/')))
}

#[async_trait(?Send)]
impl PipelineStage for RasterizeSvgImagesStage {
    fn name(&self) -> &str {
        "rasterize-svg-images"
    }

    fn input_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    fn output_kind(&self) -> PipelineDataKind {
        PipelineDataKind::DocumentAst
    }

    async fn run(
        &self,
        input: PipelineData,
        ctx: &mut StageContext,
    ) -> Result<PipelineData, PipelineError> {
        let PipelineData::DocumentAst(mut doc) = input else {
            return Err(PipelineError::unexpected_input(
                self.name(),
                self.input_kind(),
                input.kind(),
            ));
        };

        if !matches!(
            ctx.format.identifier,
            FormatIdentifier::Docx | FormatIdentifier::Pptx
        ) || !ctx.runtime.can_rasterize_svg()
        {
            return Ok(PipelineData::DocumentAst(doc));
        }

        let doc_dir = {
            let dir = doc.path.parent().unwrap_or_else(|| Path::new("."));
            if dir.has_root() || dir.is_absolute() {
                dir.to_path_buf()
            } else {
                ctx.project.dir.join(dir)
            }
        };

        // Distinct SVG targets, in document order.
        let mut targets: Vec<String> = Vec::new();
        crate::ast_walk::for_each_inline_mut(&mut doc.ast.blocks, &mut |inline| {
            if let Inline::Image(img) = inline
                && names_an_svg(&img.target.0)
                && !targets.contains(&img.target.0)
            {
                targets.push(img.target.0.clone());
            }
        });
        if targets.is_empty() {
            return Ok(PipelineData::DocumentAst(doc));
        }

        let raster_dir = doc_dir.join(RASTER_DIR);
        let project_root = normalize_request_path(&ctx.project.dir);
        let limits = constants().limits;

        // target -> mounted PNG path, or why it was left as an SVG. `None` is
        // an SVG that is not there (pandoc reports the missing file itself).
        let mut outcome: HashMap<String, Option<Result<PathBuf, String>>> = HashMap::new();
        // SVG hash -> the same, so one SVG referenced under two names (or twice)
        // is rasterized and mounted once.
        let mut by_hash: HashMap<String, Result<PathBuf, String>> = HashMap::new();
        let mut rasterized: u64 = 0;
        let mut unavailable = false;

        // One at a time: each canvas can be megapixels.
        for target in &targets {
            let Some(path) = resolve_image_target(&doc_dir, target, None) else {
                continue;
            };
            // Outside the project the collector refuses (and warns about) the
            // original target; there is nothing to read in the snapshot anyway.
            if !is_under(&normalize_request_path(&path), &project_root)
                || !ctx.runtime.is_file(&path).unwrap_or(false)
            {
                outcome.insert(target.clone(), None);
                continue;
            }
            let svg = match ctx.runtime.file_read(&path) {
                Ok(bytes) => bytes,
                Err(e) => {
                    outcome.insert(target.clone(), Some(Err(format!("could not read it: {e}"))));
                    continue;
                }
            };
            let hash = mounted_name(&svg);
            if let Some(done) = by_hash.get(&hash) {
                outcome.insert(target.clone(), Some(done.clone()));
                continue;
            }
            if unavailable {
                continue;
            }
            let result = match ctx.runtime.rasterize_svg(&svg, MAX_SIDE).await {
                Ok(png) if png.len() as u64 > limits.image_bytes => Err(format!(
                    "the PNG is {} bytes; the limit is {}",
                    png.len(),
                    limits.image_bytes
                )),
                Ok(png) if rasterized + png.len() as u64 > limits.total_bytes => Err(format!(
                    "the PNGs already made fill the {} byte limit on a browser render",
                    limits.total_bytes
                )),
                Ok(png) => {
                    let mounted = raster_dir.join(&hash);
                    ctx.runtime
                        .dir_create(&raster_dir, true)
                        .map_err(|e| format!("could not create {}: {e}", raster_dir.display()))
                        .and_then(|()| {
                            ctx.runtime
                                .file_write(&mounted, &png)
                                .map_err(|e| format!("could not store the PNG: {e}"))
                        })
                        .map(|()| {
                            rasterized += png.len() as u64;
                            mounted
                        })
                }
                // No rasterizer after all: not this SVG's failure, and not worth a warning.
                Err(RuntimeError::NotSupported(_)) => {
                    unavailable = true;
                    continue;
                }
                Err(e) => Err(e.to_string()),
            };
            by_hash.insert(hash, result.clone());
            outcome.insert(target.clone(), Some(result));
        }

        crate::ast_walk::for_each_inline_mut(&mut doc.ast.blocks, &mut |inline| {
            let Inline::Image(img) = inline else { return };
            if let Some(Some(Ok(path))) = outcome.get(&img.target.0) {
                let original = std::mem::replace(&mut img.target.0, normalize_request_path(path));
                img.attr.2.insert(RASTER_SRC_ATTR.to_string(), original);
            }
        });

        for target in &targets {
            if let Some(Some(Err(reason))) = outcome.get(target) {
                ctx.add_diagnostic(warning(format!(
                    "SVG image {target} was not converted to PNG (pandoc will show the alt text): {reason}"
                )));
            }
        }
        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_svg_files_in_any_case_and_encoding() {
        for t in [
            "a.svg",
            "dir/B.SVG",
            "sp%20ace.svg",
            "/abs/x.Svg",
            "a%2Esvg",
        ] {
            assert!(names_an_svg(t), "{t}");
        }
    }

    #[test]
    fn does_not_match_other_targets() {
        for t in [
            "a.svg?x=1",
            "a.svg#frag",
            "a.png",
            "a.svgz",
            "svg",
            "",
            "https://x.org/a.svg",
            "data:image/svg+xml;base64,AAAA",
        ] {
            assert!(!names_an_svg(t), "{t}");
        }
    }

    #[test]
    fn mounted_names_depend_on_the_bytes_alone() {
        assert_eq!(mounted_name(b"<svg/>"), mounted_name(b"<svg/>"));
        assert_ne!(mounted_name(b"<svg/>"), mounted_name(b"<svg />"));
        assert_eq!(mounted_name(b"x").len(), 16 + ".png".len());
    }

    #[test]
    fn under_is_a_path_prefix_not_a_string_prefix() {
        assert!(is_under("/p/a.svg", "/p"));
        assert!(is_under("/p/a.svg", "/p/"));
        assert!(!is_under("/pp/a.svg", "/p"));
    }
}
