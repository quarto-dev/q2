//! `PrefetchRemoteImagesStage`: pandoc.wasm cannot fetch, so before the
//! request is built every remote image the document names is downloaded
//! through the runtime, mounted as `<doc_dir>/_remote/<hash>.<ext>` and the
//! `Image`'s `src` is rewritten to that absolute path (pandoc's working
//! directory in the browser is `/`, so nothing document-relative survives).
//! The original URL stays on the image as the `q2-remote-src` attribute for
//! diagnostics.
//!
//! The stage only *adds* files, and only to the runtime it is given, which
//! in the browser is the click's snapshot. It sits just before
//! `pandoc-prepare`, which then collects the mounted files like any local
//! image. It is only in the pandoc-request stage list.
//!
//! A failed fetch leaves the URL for docx/pptx/epub (pandoc warns and shows
//! the alt text, `Q-11-1`); for typst it replaces the `Image` with its alt
//! text and warns `Q-20-9`, because `typst.lua` calls `pandoc.mediabag.fetch` on any URL left in
//! the AST and pandoc.wasm exits 83 when that fails.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures::future::join_all;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_pandoc_types::Inline;
use quarto_pandoc_types::attr::AttrSourceInfo;
use quarto_pandoc_types::inline::Span;
use quarto_system_runtime::{FetchPolicy, RuntimeError, SystemRuntime};
use sha2::{Digest, Sha256};

use crate::format::FormatIdentifier;
use crate::pandoc_request::{constants, normalize_request_path};
use crate::stage::{PipelineData, PipelineDataKind, PipelineError, PipelineStage, StageContext};

/// The attribute that keeps the URL an image was fetched from.
pub const REMOTE_SRC_ATTR: &str = "q2-remote-src";

/// The directory next to the document that holds the downloaded images.
pub const REMOTE_DIR: &str = "_remote";

/// Downloads in flight at once.
const PARALLEL_FETCHES: usize = 6;

#[derive(Default)]
pub struct PrefetchRemoteImagesStage;

impl PrefetchRemoteImagesStage {
    pub fn new() -> Self {
        Self
    }
}

/// An image target the stage fetches: `http(s)://`. `data:` URIs and
/// protocol-relative (`//host/x`) targets are pandoc's to handle or ignore;
/// everything else is a document-relative file.
pub fn is_remote_image_url(target: &str) -> bool {
    let lower = target.trim_start().to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// A file extension for a response: from its MIME type, else from the URL's
/// own extension when that names an image.
pub fn image_extension(mime: &str, url: &str) -> Option<&'static str> {
    let essence = mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let from_mime = match essence.as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" | "image/pjpeg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/svg+xml" => Some("svg"),
        "image/webp" => Some("webp"),
        "image/avif" => Some("avif"),
        "image/bmp" | "image/x-ms-bmp" => Some("bmp"),
        "image/tiff" => Some("tif"),
        "image/x-icon" | "image/vnd.microsoft.icon" => Some("ico"),
        "image/emf" | "image/x-emf" => Some("emf"),
        "image/wmf" | "image/x-wmf" => Some("wmf"),
        "application/pdf" => Some("pdf"),
        _ => None,
    };
    if from_mime.is_some() {
        return from_mime;
    }
    let path = url.split(['?', '#']).next().unwrap_or("");
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    [
        "png", "jpg", "jpeg", "gif", "svg", "webp", "avif", "bmp", "tif", "tiff", "ico", "emf",
        "wmf", "pdf",
    ]
    .into_iter()
    .find(|e| *e == ext)
    .map(|e| if e == "jpeg" { "jpg" } else { e })
}

/// `<hash>.<ext>`: the first 16 hex digits of the URL's SHA-256.
fn mounted_name(url: &str, ext: &str) -> String {
    format!(
        "{}.{ext}",
        &hex::encode(Sha256::digest(url.as_bytes()))[..16]
    )
}

fn warning(message: String, code: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(message)
        .with_code(code)
        .build()
}

/// Fetch `url`, mount it under `remote_dir` and return the mounted path.
async fn fetch_and_mount(
    runtime: &dyn SystemRuntime,
    url: &str,
    remote_dir: &Path,
    policy: &FetchPolicy,
) -> Result<PathBuf, String> {
    let (bytes, mime) = runtime
        .fetch_url_hardened(url, policy)
        .await
        .map_err(|e| match e {
            RuntimeError::Network(m) | RuntimeError::NotSupported(m) => m,
            other => other.to_string(),
        })?;
    let ext = image_extension(&mime, url)
        .ok_or_else(|| format!("{url} is `{mime}`, which is not an image type pandoc reads"))?;
    runtime
        .dir_create(remote_dir, true)
        .map_err(|e| format!("could not create {}: {e}", remote_dir.display()))?;
    let path = remote_dir.join(mounted_name(url, ext));
    runtime
        .file_write(&path, &bytes)
        .map_err(|e| format!("could not store {url}: {e}"))?;
    Ok(path)
}

#[async_trait(?Send)]
impl PipelineStage for PrefetchRemoteImagesStage {
    fn name(&self) -> &str {
        "prefetch-remote-images"
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

        // Distinct remote targets, in document order.
        let mut urls: Vec<String> = Vec::new();
        crate::ast_walk::for_each_inline_mut(&mut doc.ast.blocks, &mut |inline| {
            if let Inline::Image(img) = inline
                && is_remote_image_url(&img.target.0)
                && !urls.contains(&img.target.0)
            {
                urls.push(img.target.0.clone());
            }
        });
        if urls.is_empty() {
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
        let remote_dir = doc_dir.join(REMOTE_DIR);
        let limits = constants().limits;
        let policy = FetchPolicy {
            max_bytes: limits.image_bytes,
            ..FetchPolicy::default()
        };

        // url -> mounted path, or why it could not be fetched.
        let mut outcome: HashMap<String, Result<PathBuf, String>> = HashMap::new();
        let mut fetched: u64 = 0;
        for chunk in urls.chunks(PARALLEL_FETCHES) {
            if fetched > limits.total_bytes {
                for url in chunk {
                    outcome.insert(
                        url.clone(),
                        Err(format!(
                            "{url} was not fetched: the images already fetched fill the {} byte limit on a browser render",
                            limits.total_bytes
                        )),
                    );
                }
                continue;
            }
            let results = join_all(
                chunk
                    .iter()
                    .map(|url| fetch_and_mount(ctx.runtime.as_ref(), url, &remote_dir, &policy)),
            )
            .await;
            for (url, result) in chunk.iter().zip(results) {
                if let Ok(path) = &result {
                    fetched += ctx.runtime.path_metadata(path).map_or(0, |m| m.size);
                }
                outcome.insert(url.clone(), result);
            }
        }

        let typst = ctx.format.identifier == FormatIdentifier::Typst;
        crate::ast_walk::for_each_inline_mut(&mut doc.ast.blocks, &mut |inline| {
            let Inline::Image(img) = inline else { return };
            let Some(result) = outcome.get(&img.target.0) else {
                return;
            };
            match result {
                Ok(path) => {
                    let original =
                        std::mem::replace(&mut img.target.0, normalize_request_path(path));
                    img.attr.2.insert(REMOTE_SRC_ATTR.to_string(), original);
                }
                Err(_) if typst => {
                    // typst.lua would fetch the URL again and exit 83 on failure.
                    *inline = Inline::Span(Span {
                        attr: Default::default(),
                        content: std::mem::take(&mut img.content),
                        source_info: img.source_info.clone(),
                        attr_source: AttrSourceInfo::empty(),
                    });
                }
                Err(_) => {}
            }
        });

        for url in &urls {
            if let Some(Err(reason)) = outcome.get(url) {
                let what = if typst {
                    "its alt text is used instead"
                } else {
                    "pandoc will show the alt text"
                };
                // The reasons name the URL unless the network layer's did not.
                let reason = if reason.contains(url.as_str()) {
                    reason.clone()
                } else {
                    format!("{url}: {reason}")
                };
                ctx.add_diagnostic(warning(
                    format!("Remote image not included ({what}): {reason}"),
                    // Typst's own code; docx/pptx/epub share the resource one
                    // pandoc's warning for the same image carries.
                    if typst { "Q-20-9" } else { "Q-11-1" },
                ));
            }
        }
        Ok(PipelineData::DocumentAst(doc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_means_http_or_https() {
        for url in ["https://x.org/a.png", "HTTP://x.org/a.png", "  https://x/a"] {
            assert!(is_remote_image_url(url), "{url}");
        }
        for url in [
            "a.png",
            "./a.png",
            "/a.png",
            "//x.org/a.png",
            "data:image/png;base64,AAAA",
            "ftp://x.org/a.png",
            "file:///a.png",
            "",
        ] {
            assert!(!is_remote_image_url(url), "{url:?}");
        }
    }

    #[test]
    fn extension_prefers_mime_over_url() {
        assert_eq!(image_extension("image/png", "https://x/a.jpg"), Some("png"));
        assert_eq!(
            image_extension("image/jpeg; charset=binary", "https://x/a"),
            Some("jpg")
        );
        assert_eq!(image_extension("image/svg+xml", "https://x/a"), Some("svg"));
        assert_eq!(
            image_extension("application/octet-stream", "https://x/a.PNG?v=2#f"),
            Some("png")
        );
        assert_eq!(
            image_extension("application/octet-stream", "https://x/a.jpeg"),
            Some("jpg")
        );
        assert_eq!(image_extension("text/html", "https://x/a"), None);
        assert_eq!(image_extension("text/html", "https://x/a.html"), None);
    }

    #[test]
    fn mounted_names_are_stable_and_distinct() {
        let a = mounted_name("https://x.org/a.png", "png");
        assert_eq!(a, mounted_name("https://x.org/a.png", "png"));
        assert_ne!(a, mounted_name("https://x.org/b.png", "png"));
        assert_eq!(a.len(), 16 + ".png".len());
    }
}
