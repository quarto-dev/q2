/*
 * project/listing/post_render_upgrade/substitute.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * L7 substitution: walk every project output, regex-match the
 * description / image envelopes emitted by L3, replace them with
 * engine-rendered preview content from sibling outputs (or the L1
 * fallback when no engine content is available).
 */

//! Listing-placeholder substitution.
//!
//! Called once per project from `WebsiteProjectType::post_render`,
//! after every per-file Pass-2 has finished writing. Reads each
//! output file, regex-finds the L3 envelopes, reads the referenced
//! sibling output (cached per call), and rewrites the host file in
//! place with the substituted preview content.
//!
//! See parent module [`super`]'s header for the bracketing rules
//! that gate this whole subsystem.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use quarto_error_reporting::{DiagnosticMessage, DiagnosticMessageBuilder};
use quarto_source_map::{By, SourceInfo};
use quarto_system_runtime::{RuntimeError, SystemRuntime};
use regex::RegexBuilder;

use crate::Result;
use crate::error::QuartoError;
use crate::project::ProjectContext;
use crate::project::listing::placeholders::{DESC_REGEX, IMG_REGEX};

use super::reader::{PreviewImage, ReaderOptions, RenderedExtraction, extract};

/// Walk every output file, replacing description / image envelope
/// markers with engine-rendered previews from sibling outputs.
///
/// Files without any envelope marker are skipped via a quick
/// byte-search; only files that contain at least one marker pay
/// the regex/parse cost.
///
/// I/O errors other than `NotFound` propagate up and abort
/// `post_render` (per plan §"Risks and mitigations" #6: silently
/// swallowing read errors would mask filesystem misconfiguration).
/// `NotFound` on a sibling output file emits `Q-12-13` and the L1
/// fallback is retained.
pub fn substitute_listing_placeholders(
    project: &ProjectContext,
    output_paths: &[PathBuf],
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    // Compiled regexes (one set per call; the cost is in the µs
    // range and these never escape the function).
    let desc_re = RegexBuilder::new(DESC_REGEX)
        .dot_matches_new_line(true)
        .build()
        .map_err(|e| QuartoError::other(format!("DESC_REGEX failed to compile: {e}")))?;
    let img_re = RegexBuilder::new(IMG_REGEX)
        .dot_matches_new_line(true)
        .build()
        .map_err(|e| QuartoError::other(format!("IMG_REGEX failed to compile: {e}")))?;

    // Per-call cache keyed on absolute sibling path. Value is
    // `None` when the sibling was missing (cached so we don't
    // re-stat); `Some(extraction)` otherwise.
    let mut cache: HashMap<PathBuf, Option<RenderedExtraction>> = HashMap::new();

    for host_path in output_paths {
        let bytes = match runtime.file_read(host_path) {
            Ok(b) => b,
            Err(RuntimeError::Io(e)) if e.kind() == ErrorKind::NotFound => {
                // Output orchestrator-reported but missing on disk —
                // can't substitute what isn't there. Skip silently.
                continue;
            }
            Err(e) => {
                return Err(QuartoError::other(format!(
                    "L7 post_render: failed to read {}: {}",
                    host_path.display(),
                    e
                )));
            }
        };
        let host_html = match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => continue, // Non-UTF-8 output: skip silently.
        };

        // Quick byte-search to skip files that don't contain any
        // envelope marker. memmem-style; effectively free.
        if !host_html.contains("<!-- desc-begin(") && !host_html.contains("<!-- img-begin(") {
            continue;
        }

        // Substitute descriptions and images. Both are run in
        // sequence; the second sees the (possibly already-mutated)
        // text. Each substitution may trigger sibling reads (cached
        // by absolute path).
        let after_desc = substitute_descriptions(
            &host_html,
            host_path,
            project,
            runtime,
            &desc_re,
            &mut cache,
            diagnostics,
        )?;
        let after_img = substitute_images(
            &after_desc,
            host_path,
            project,
            runtime,
            &img_re,
            &mut cache,
        )?;

        // Only rewrite the file if anything changed.
        if after_img != host_html {
            runtime
                .file_write(host_path, after_img.as_bytes())
                .map_err(|e| {
                    QuartoError::other(format!(
                        "L7 post_render: failed to write {}: {}",
                        host_path.display(),
                        e
                    ))
                })?;
        }
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────
// Description substitution
// ─────────────────────────────────────────────────────────────────

fn substitute_descriptions(
    host_html: &str,
    _host_path: &Path,
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    re: &regex::Regex,
    cache: &mut HashMap<PathBuf, Option<RenderedExtraction>>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<String> {
    let mut out = String::with_capacity(host_html.len());
    let mut last_end = 0usize;

    for caps in re.captures_iter(host_html) {
        // Capture groups (DESC_REGEX): 1=max, 2=href, 3=inner.
        let m = caps.get(0).expect("whole match always present");
        let max_str = caps.get(1).map_or("0", |c| c.as_str());
        let href = caps.get(2).map_or("", |c| c.as_str()).to_string();
        let inner = caps.get(3).map_or("", |c| c.as_str());
        let max_length: usize = max_str.parse().unwrap_or(0);

        // Append everything before this match.
        out.push_str(&host_html[last_end..m.start()]);

        // Resolve sibling path (project.output_dir + href).
        let sibling_abs = project.output_dir.join(&href);
        let extraction = read_or_cached(&sibling_abs, runtime, cache).map_err(|e| {
            QuartoError::other(format!(
                "L7 post_render: failed to read sibling {}: {}",
                sibling_abs.display(),
                e
            ))
        })?;

        let replacement = match extraction {
            Some(rx) => match rx.first_para_html.as_deref() {
                Some(s) if !s.is_empty() => {
                    let opts = ReaderOptions {
                        max_length: if max_length == 0 {
                            None
                        } else {
                            Some(max_length)
                        },
                        ..Default::default()
                    };
                    // Re-extract with this envelope's max_length —
                    // the cached extraction was computed without
                    // max_length so the cached preview is full-text.
                    // For per-envelope truncation we re-run extract
                    // on the cached HTML. (See plan §Determinism;
                    // re-extraction is deterministic given input.)
                    if let Some(html) = rx.cached_html.as_deref().map(|h| extract(h, &opts)) {
                        html.first_para_html.unwrap_or_else(|| s.to_string())
                    } else {
                        s.to_string()
                    }
                }
                _ => {
                    diagnostics.push(make_q_12_13(&href));
                    inner.to_string()
                }
            },
            None => {
                // Sibling missing.
                diagnostics.push(make_q_12_13(&href));
                inner.to_string()
            }
        };

        out.push_str(&replacement);
        last_end = m.end();
    }

    out.push_str(&host_html[last_end..]);
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────
// Image substitution
// ─────────────────────────────────────────────────────────────────

fn substitute_images(
    host_html: &str,
    host_path: &Path,
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    re: &regex::Regex,
    cache: &mut HashMap<PathBuf, Option<RenderedExtraction>>,
) -> Result<String> {
    let mut out = String::with_capacity(host_html.len());
    let mut last_end = 0usize;

    for caps in re.captures_iter(host_html) {
        // Capture groups (IMG_REGEX): 1=attrs, 2=listing-id, 3=idx,
        // 4=href, 5=b64-default, 6=inner.
        let m = caps.get(0).expect("whole match always present");
        let attrs = caps.get(1).map_or("", |c| c.as_str());
        let _listing_id = caps.get(2).map_or("", |c| c.as_str());
        let _idx = caps.get(3).map_or("", |c| c.as_str());
        let href = caps.get(4).map_or("", |c| c.as_str()).to_string();
        let b64_default = caps.get(5).map_or("", |c| c.as_str());
        let inner = caps.get(6).map_or("", |c| c.as_str());

        // Append before-match chunk.
        out.push_str(&host_html[last_end..m.start()]);

        // Resolve sibling path.
        let sibling_abs = project.output_dir.join(&href);
        let extraction = read_or_cached(&sibling_abs, runtime, cache).map_err(|e| {
            QuartoError::other(format!(
                "L7 post_render: failed to read sibling {}: {}",
                sibling_abs.display(),
                e
            ))
        })?;

        let preview = extraction.as_ref().and_then(|rx| rx.preview_image.clone());

        let replacement = if let Some(pi) = preview {
            // Engine-rendered preview image found.
            let resolved = resolve_preview_url(host_path, &sibling_abs, &pi.src);
            build_thumbnail_img(&resolved, &pi, attrs)
        } else if !b64_default.is_empty() {
            // Listing default URL configured. The b64 alphabet is
            // URL_SAFE_NO_PAD; decode and use as-is (it's a URL
            // already, no relativization needed since the listing
            // config is authored relative to the project root).
            match URL_SAFE_NO_PAD.decode(b64_default.as_bytes()) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(url) if !url.is_empty() => {
                        let placeholder_pi = PreviewImage {
                            src: url.clone(),
                            alt: None,
                            title: None,
                        };
                        build_thumbnail_img(&url, &placeholder_pi, attrs)
                    }
                    _ => inner.to_string(),
                },
                Err(_) => inner.to_string(),
            }
        } else {
            // No preview, no default — keep the empty placeholder
            // div. **No Q-12-13** for this case (Q1 is silent here
            // too; image-less posts are routine).
            inner.to_string()
        };

        out.push_str(&replacement);
        last_end = m.end();
    }

    out.push_str(&host_html[last_end..]);
    Ok(out)
}

/// Build the substituted `<img>` tag. v1 emits a fixed shape
/// matching Q1's `progressive=false, height=, lazy=true` defaults;
/// the `attrs` field from the marker is preserved verbatim as
/// extra attributes (placeholder for follow-ups that wire
/// `listing.image-height` etc. through).
fn build_thumbnail_img(src: &str, pi: &PreviewImage, _attrs: &str) -> String {
    let alt = pi.alt.as_deref().unwrap_or("").replace('"', "&quot;");
    let lazy = r#" loading="lazy""#;
    format!(
        r#"<img src="{}" class="thumbnail-image" alt="{}"{}>"#,
        escape_attr(src),
        alt,
        lazy
    )
}

fn escape_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Compute a host-relative URL for a sibling's preview image src.
///
/// - `host_path`: absolute path of the host page (rendered file
///   currently being substituted).
/// - `sibling_abs`: absolute path of the sibling output file.
/// - `preview_src`: the `src` attribute of the preview `<img>` in
///   the sibling. May be relative (to sibling's dir), absolute
///   path-style (`/leading-slash`), absolute URL (`http://...`),
///   or a `data:` URI.
///
/// Returns the URL to embed in the host page's `<img src=...>`.
fn resolve_preview_url(host_path: &Path, sibling_abs: &Path, preview_src: &str) -> String {
    if is_absolute_url(preview_src) {
        return preview_src.to_string();
    }
    let host_dir = host_path.parent().unwrap_or(Path::new(""));
    let sibling_dir = sibling_abs.parent().unwrap_or(Path::new(""));
    // For relative `preview_src`, the absolute target is
    // `sibling_dir/preview_src`. For `/leading-slash`, treat as
    // already-absolute path (rare in Q2 output; pass through).
    if preview_src.starts_with('/') {
        return preview_src.to_string();
    }
    let preview_abs = sibling_dir.join(preview_src);
    pathdiff::diff_paths(&preview_abs, host_dir).map_or_else(
        || preview_src.to_string(),
        |p| {
            // Normalize separators to forward slashes for URL.
            p.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(os) => os.to_str().map(str::to_string),
                    std::path::Component::ParentDir => Some("..".to_string()),
                    std::path::Component::CurDir => None,
                    std::path::Component::RootDir => None,
                    std::path::Component::Prefix(_) => None,
                })
                .collect::<Vec<_>>()
                .join("/")
        },
    )
}

fn is_absolute_url(s: &str) -> bool {
    s.starts_with("http://")
        || s.starts_with("https://")
        || s.starts_with("data:")
        || s.starts_with("//")
        || s.starts_with("mailto:")
}

// ─────────────────────────────────────────────────────────────────
// Cache + diagnostics helpers
// ─────────────────────────────────────────────────────────────────

/// Read+extract `path`, caching the result by absolute path.
/// Returns `Ok(None)` when the file is missing (cached as a miss
/// so subsequent envelopes referencing the same sibling don't
/// re-stat); `Ok(Some(extraction))` on success. Other I/O errors
/// surface as `Err`.
fn read_or_cached(
    path: &Path,
    runtime: &dyn SystemRuntime,
    cache: &mut HashMap<PathBuf, Option<RenderedExtraction>>,
) -> std::result::Result<Option<RenderedExtraction>, RuntimeError> {
    let key = path.to_path_buf();
    if let Some(cached) = cache.get(&key) {
        return Ok(cached.clone());
    }
    let extraction = match runtime.file_read(path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(html) => {
                // Run extraction with no truncation; per-envelope
                // truncation re-runs `extract` against the cached
                // HTML stored on the extraction.
                let mut rx = extract(&html, &ReaderOptions::default());
                rx.cached_html = Some(html);
                Some(rx)
            }
            Err(_) => None,
        },
        Err(RuntimeError::Io(e)) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => {
            return Err(e);
        }
    };
    cache.insert(key, extraction.clone());
    Ok(extraction)
}

fn make_q_12_13(href: &str) -> DiagnosticMessage {
    DiagnosticMessageBuilder::warning(format!(
        "Listing item from {href} produced no preview content; using static fallback description."
    ))
    .with_code("Q-12-13")
    .with_location(SourceInfo::generated(By::unknown()))
    .build()
}

// ─────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectConfig;
    use crate::project::listing::placeholders;
    use quarto_system_runtime::NativeRuntime;
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    /// Build a minimal ProjectContext rooted in a tempdir.
    fn make_project(output_dir: &Path) -> ProjectContext {
        ProjectContext {
            dir: output_dir.to_path_buf(),
            config: ProjectConfig::default(),
            is_single_file: false,
            files: vec![],
            output_dir: output_dir.to_path_buf(),

            ..Default::default()
        }
    }

    /// Build a description envelope for an item with given href +
    /// max_length, wrapping `inner` between the begin/end markers.
    fn desc_envelope(href: &str, max_length: u32, inner: &str) -> String {
        format!(
            "{}\n{}\n{}",
            placeholders::description_placeholder_begin("listing-1", max_length, href),
            inner,
            placeholders::description_placeholder_end()
        )
    }

    fn img_envelope(href: &str, b64_default: &str, inner: &str) -> String {
        format!(
            "{}\n{}\n{}",
            placeholders::image_placeholder_begin(
                "listing-1",
                0,
                href,
                "progressive=false, height=, lazy=true",
                b64_default
            ),
            inner,
            placeholders::image_placeholder_end()
        )
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn read_file(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    // L7 plan §"Tests" Phase 4 #26
    #[test]
    fn substitute_description_replaces_envelope_with_engine_first_para() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("foo.html");

        let l1 = "<p>L1 fallback paragraph.</p>";
        let host = format!(
            "<html><body>{}</body></html>",
            desc_envelope("posts/foo.html", 0, l1)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>Engine first para.</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains("Engine first para."),
            "expected engine first para in host; got: {after}"
        );
        assert!(
            !after.contains("desc-begin(5A0113B34292)"),
            "expected begin marker stripped"
        );
        assert!(
            !after.contains("desc-end(5A0113B34292)"),
            "expected end marker stripped"
        );
        assert!(
            !after.contains(l1),
            "expected L1 fallback replaced (not retained)"
        );
        assert!(diags.is_empty(), "expected no diagnostics, got {:?}", diags);
    }

    // L7 plan §"Tests" Phase 4 #27
    #[test]
    fn substitute_description_keeps_l1_when_sibling_first_para_empty() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("bar.html");

        let l1 = "<p>L1 fallback paragraph for bar.</p>";
        let host = format!(
            "<html><body>{}</body></html>",
            desc_envelope("posts/bar.html", 0, l1)
        );
        write_file(&host_path, &host);
        // Sibling has no <main class="content"> → first_para is None.
        write_file(
            &sibling_path,
            r#"<html><body><p>Outside main</p></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains(l1),
            "expected L1 fallback retained; got: {after}"
        );
        assert!(
            !after.contains("desc-begin"),
            "expected begin marker stripped"
        );
        assert!(!after.contains("desc-end"), "expected end marker stripped");
        assert_eq!(diags.len(), 1, "expected one Q-12-13");
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-13"));
    }

    // L7 plan §"Tests" Phase 4 #28
    #[test]
    fn substitute_description_keeps_l1_when_sibling_missing() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");

        let l1 = "<p>L1 fallback for missing sibling.</p>";
        let host = format!(
            "<html><body>{}</body></html>",
            desc_envelope("posts/missing.html", 0, l1)
        );
        write_file(&host_path, &host);
        // No sibling file written.

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains(l1),
            "expected L1 fallback retained when sibling missing; got: {after}"
        );
        assert!(!after.contains("desc-begin"));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("Q-12-13"));
    }

    // L7 plan §"Tests" Phase 4 #29
    #[test]
    fn substitute_description_truncates_to_max_from_marker() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("long.html");

        let l1 = "<p>L1 fallback.</p>";
        let host = format!(
            "<html><body>{}</body></html>",
            desc_envelope("posts/long.html", 20, l1) // max=20
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>The quick brown fox jumps over the lazy dog repeatedly.</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        // Substituted text follows Q1's `truncateText(s, 20,
        // "space")` (bd-listing-ellipsis-no-matching-l963osy1):
        // first 20 chars "The quick brown fox ", drop one, cut at
        // the last space, append `…`. The full sentence should not
        // appear.
        assert!(
            after.contains("The quick brown…"),
            "expected truncated preview with ellipsis present; got: {after}"
        );
        assert!(
            !after.contains("fox jumps over the lazy"),
            "expected post-truncation text absent; got: {after}"
        );
        assert!(diags.is_empty());
    }

    // L7 plan §"Tests" Phase 4 #30
    #[test]
    fn substitute_description_handles_multiple_envelopes_one_file() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let foo_path = temp.path().join("posts").join("foo.html");
        let bar_path = temp.path().join("posts").join("bar.html");

        let host = format!(
            "<html><body>{}\n\n{}</body></html>",
            desc_envelope("posts/foo.html", 0, "<p>L1 foo.</p>"),
            desc_envelope("posts/bar.html", 0, "<p>L1 bar.</p>"),
        );
        write_file(&host_path, &host);
        write_file(
            &foo_path,
            r#"<html><body><main class="content"><p>Engine FOO.</p></main></body></html>"#,
        );
        write_file(
            &bar_path,
            r#"<html><body><main class="content"><p>Engine BAR.</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(after.contains("Engine FOO."));
        assert!(after.contains("Engine BAR."));
        assert!(!after.contains("desc-begin"));
        assert!(diags.is_empty());
    }

    // L7 plan §"Tests" Phase 4 #31
    #[test]
    fn substitute_image_replaces_envelope_with_preview_img() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("p.html");

        let inner = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let host = format!(
            "<html><body>{}</body></html>",
            img_envelope("posts/p.html", "", inner)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content">
<img src="figures/preview-image.png">
</main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains(r#"class="thumbnail-image""#),
            "expected substituted thumbnail; got: {after}"
        );
        // src is resolved relative to host: from index.html →
        // posts/figures/preview-image.png.
        assert!(
            after.contains(r#"src="posts/figures/preview-image.png""#),
            "expected resolved src; got: {after}"
        );
        assert!(!after.contains("img-begin"));
        assert!(!after.contains("listing-item-img-placeholder"));
    }

    // L7 plan §"Tests" Phase 4 #32
    #[test]
    fn substitute_image_uses_listing_default_when_no_preview() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("noimg.html");

        let default_url = "assets/site/default.png";
        let b64 = URL_SAFE_NO_PAD.encode(default_url.as_bytes());

        let inner = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let host = format!(
            "<html><body>{}</body></html>",
            img_envelope("posts/noimg.html", &b64, inner)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>No images here.</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains(&format!(r#"src="{}""#, default_url)),
            "expected listing-default URL substituted; got: {after}"
        );
        assert!(after.contains(r#"class="thumbnail-image""#));
        assert!(!after.contains("img-begin"));
    }

    // L7 plan §"Tests" Phase 4 #33
    #[test]
    fn substitute_image_keeps_empty_div_when_no_preview_no_default() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("plain.html");

        let inner = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let host = format!(
            "<html><body>{}</body></html>",
            img_envelope("posts/plain.html", "", inner)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>Plain text only.</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains("listing-item-img-placeholder"),
            "expected empty placeholder retained; got: {after}"
        );
        assert!(!after.contains("img-begin"));
    }

    // L7 plan §"Tests" Phase 4 #34 — already partially covered by
    // #31, but explicit assertion on the host-relative resolution.
    #[test]
    fn substitute_image_resolves_src_relative_to_host_output_dir() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        // Host at root, sibling under posts/, image under posts/figures/
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("foo.html");

        let inner = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let host = format!(
            "<html><body>{}</body></html>",
            img_envelope("posts/foo.html", "", inner)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content">
<img src="figures/foo.png" class="preview-image">
</main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        let after = read_file(&host_path);
        assert!(
            after.contains(r#"src="posts/figures/foo.png""#),
            "expected host-relative src `posts/figures/foo.png`; got: {after}"
        );
    }

    // L7 plan §"Tests" Phase 4 #35
    #[test]
    fn substitute_image_no_warning_when_no_preview() {
        // The "no preview" path doesn't emit Q-12-13. (Q-12-13 is
        // description-only.)
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("plain.html");

        let inner = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let host = format!(
            "<html><body>{}</body></html>",
            img_envelope("posts/plain.html", "", inner)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>plain</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        // No Q-12-13 for the image path even though no preview was
        // found; description path didn't fire either (no desc
        // envelope in the fixture).
        assert!(
            diags.iter().all(|d| d.code.as_deref() != Some("Q-12-13")),
            "expected no Q-12-13 from image-only no-preview case; got: {:?}",
            diags
        );
    }

    /// Counting wrapper for cache tests. Forwards every
    /// [`SystemRuntime`] method to an inner [`NativeRuntime`] but
    /// increments `reads` on each `file_read` call.
    struct CountingRuntime {
        inner: NativeRuntime,
        reads: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl SystemRuntime for CountingRuntime {
        fn file_read(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.inner.file_read(p)
        }
        fn file_write(&self, p: &Path, c: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_write(p, c)
        }
        fn path_exists(
            &self,
            p: &Path,
            k: Option<quarto_system_runtime::PathKind>,
        ) -> quarto_system_runtime::RuntimeResult<bool> {
            self.inner.path_exists(p, k)
        }
        fn canonicalize(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.canonicalize(p)
        }
        fn path_metadata(
            &self,
            p: &Path,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::PathMetadata> {
            self.inner.path_metadata(p)
        }
        fn file_copy(&self, s: &Path, d: &Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_copy(s, d)
        }
        fn path_rename(&self, o: &Path, n: &Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.path_rename(o, n)
        }
        fn file_remove(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.file_remove(p)
        }
        fn dir_create(&self, p: &Path, r: bool) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.dir_create(p, r)
        }
        fn dir_remove(&self, p: &Path, r: bool) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.dir_remove(p, r)
        }
        fn dir_list(&self, p: &Path) -> quarto_system_runtime::RuntimeResult<Vec<PathBuf>> {
            self.inner.dir_list(p)
        }
        fn cwd(&self) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.cwd()
        }
        fn temp_dir(
            &self,
            t: &str,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::TempDir> {
            self.inner.temp_dir(t)
        }
        fn exec_pipe(
            &self,
            c: &str,
            a: &[&str],
            s: &[u8],
        ) -> quarto_system_runtime::RuntimeResult<Vec<u8>> {
            self.inner.exec_pipe(c, a, s)
        }
        fn exec_command(
            &self,
            c: &str,
            a: &[&str],
            s: Option<&[u8]>,
        ) -> quarto_system_runtime::RuntimeResult<quarto_system_runtime::CommandOutput> {
            self.inner.exec_command(c, a, s)
        }
        fn env_get(&self, n: &str) -> quarto_system_runtime::RuntimeResult<Option<String>> {
            self.inner.env_get(n)
        }
        fn env_all(
            &self,
        ) -> quarto_system_runtime::RuntimeResult<std::collections::HashMap<String, String>>
        {
            self.inner.env_all()
        }
        async fn fetch_url(
            &self,
            u: &str,
        ) -> quarto_system_runtime::RuntimeResult<(Vec<u8>, String)> {
            self.inner.fetch_url(u).await
        }
        fn os_name(&self) -> &'static str {
            self.inner.os_name()
        }
        fn arch(&self) -> &'static str {
            self.inner.arch()
        }
        fn cpu_time(&self) -> quarto_system_runtime::RuntimeResult<u64> {
            self.inner.cpu_time()
        }
        fn xdg_dir(
            &self,
            k: quarto_system_runtime::XdgDirKind,
            s: Option<&Path>,
        ) -> quarto_system_runtime::RuntimeResult<PathBuf> {
            self.inner.xdg_dir(k, s)
        }
        fn stdout_write(&self, d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.stdout_write(d)
        }
        fn stderr_write(&self, d: &[u8]) -> quarto_system_runtime::RuntimeResult<()> {
            self.inner.stderr_write(d)
        }
    }

    // L7 plan §"Tests" Phase 4 #36
    #[test]
    fn substitute_caches_sibling_extraction_within_one_call() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("foo.html");

        // Same sibling referenced by both desc and img envelopes —
        // should be read exactly once.
        let inner_img = r#"<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>"#;
        let host = format!(
            "<html><body>{}\n{}</body></html>",
            desc_envelope("posts/foo.html", 0, "<p>L1.</p>"),
            img_envelope("posts/foo.html", "", inner_img)
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>Engine para.</p>
<img src="preview.png" class="preview-image">
</main></body></html>"#,
        );

        let reads = Arc::new(AtomicUsize::new(0));
        let runtime = CountingRuntime {
            inner: NativeRuntime::new(),
            reads: reads.clone(),
        };
        let mut diags = vec![];
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();

        // Reads: 1 for the host file + 1 for the sibling (cached
        // across the two envelopes referencing it). Total: 2.
        let total = reads.load(Ordering::SeqCst);
        assert_eq!(
            total, 2,
            "expected exactly 2 reads (host + sibling-once); got: {}",
            total
        );
    }

    // L7 plan §"Tests" Phase 4 #37
    #[test]
    fn substitute_does_not_cache_across_calls() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_path = temp.path().join("index.html");
        let sibling_path = temp.path().join("posts").join("foo.html");

        let host = format!(
            "<html><body>{}</body></html>",
            desc_envelope("posts/foo.html", 0, "<p>L1.</p>")
        );
        write_file(&host_path, &host);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>Para.</p></main></body></html>"#,
        );

        let reads = Arc::new(AtomicUsize::new(0));
        let runtime = CountingRuntime {
            inner: NativeRuntime::new(),
            reads: reads.clone(),
        };
        let mut diags = vec![];
        // First call.
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();
        let after_first = reads.load(Ordering::SeqCst);

        // Second call. Should re-read everything.
        substitute_listing_placeholders(&project, &[host_path.clone()], &runtime, &mut diags)
            .unwrap();
        let after_second = reads.load(Ordering::SeqCst);

        // The second call adds reads for at least the host (the
        // file on disk no longer contains the envelope after the
        // first call's substitution, so the sibling won't be read
        // a second time — but the host certainly is).
        assert!(
            after_second > after_first,
            "second call should perform new reads (no static cache); first={}, second={}",
            after_first,
            after_second
        );
    }

    // Drift guard: two listing hosts referencing the same sibling
    // both substitute correctly.
    #[test]
    fn substitute_two_hosts_same_sibling_both_substitute() {
        let temp = TempDir::new().unwrap();
        let project = make_project(temp.path());
        let host_a = temp.path().join("a.html");
        let host_b = temp.path().join("b.html");
        let sibling_path = temp.path().join("posts").join("shared.html");

        let host_html = format!(
            "<html><body>{}</body></html>",
            desc_envelope("posts/shared.html", 0, "<p>L1.</p>")
        );
        write_file(&host_a, &host_html);
        write_file(&host_b, &host_html);
        write_file(
            &sibling_path,
            r#"<html><body><main class="content"><p>Shared engine para.</p></main></body></html>"#,
        );

        let runtime = NativeRuntime::new();
        let mut diags = vec![];
        substitute_listing_placeholders(
            &project,
            &[host_a.clone(), host_b.clone()],
            &runtime,
            &mut diags,
        )
        .unwrap();

        assert!(read_file(&host_a).contains("Shared engine para."));
        assert!(read_file(&host_b).contains("Shared engine para."));
    }
}
