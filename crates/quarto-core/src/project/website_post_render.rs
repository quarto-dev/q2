/*
 * project/website_post_render.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * WebsiteProjectType post-render hooks: favicon copy, sitemap.xml,
 * robots.txt.
 */

//! Post-render hooks for [`WebsiteProjectType`].
//!
//! Each function below runs once per project, after Pass 2 has
//! finished rendering every file. The three hooks are:
//!
//! - [`copy_favicon`] (Phase 7): copy `<project>/favicon-path` to
//!   `<output_dir>/favicon-path`.
//! - [`write_sitemap`] (Phase 7): emit `_site/sitemap.xml` when
//!   `website.site-url` is set.
//! - [`write_robots_txt`] (Phase 7): emit `_site/robots.txt`
//!   pointing at the sitemap, unless the user provided one.
//!
//! The Phase 5 project-artifact flush used to live here too, as
//! `flush_site_libs`. bd-v8gx moved it to
//! [`crate::artifact_flush::flush_project_artifacts`]: two of its three
//! callers were not website renders, and the write loop is shared with
//! the rest of the artifact-write family.
//!
//! Every hook that remains here is native-only — each writes into the
//! project's on-disk output directory, which does not exist in the
//! in-browser preview.
//!
//! Each hook short-circuits cleanly when its triggering config is
//! absent. Failures bubble up as
//! [`crate::error::QuartoError`]s — by Phase 1's contract, hook
//! failures abort the whole project render.
//!
//! See `claude-notes/plans/2026-04-27-websites-phase-7.md`
//! Decisions 8–11.
//!
//! [`WebsiteProjectType`]: super::orchestrator::WebsiteProjectType

#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::time::SystemTime;

#[cfg(not(target_arch = "wasm32"))]
use quarto_error_reporting::DiagnosticMessage;
#[cfg(not(target_arch = "wasm32"))]
use quarto_system_runtime::SystemRuntime;

#[cfg(not(target_arch = "wasm32"))]
use crate::Result;
#[cfg(not(target_arch = "wasm32"))]
use crate::error::QuartoError;
#[cfg(not(target_arch = "wasm32"))]
use crate::project::ProjectContext;
#[cfg(not(target_arch = "wasm32"))]
use crate::project::index::ProjectIndex;
#[cfg(not(target_arch = "wasm32"))]
use crate::project::website_config::{resolved_website_favicon, website_site_url};

// ═══════════════════════════════════════════════════════════════════
// Favicon (Phase 7) — native-only
// ═══════════════════════════════════════════════════════════════════

/// Copy the user-configured favicon from the project root to the
/// output directory.
///
/// No-op when:
/// - `website.favicon` is not set.
/// - `project.config.metadata` is unavailable (single-doc render).
/// - The source file is missing — emits a warning diagnostic into
///   `diagnostics` but does not error so the render completes.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn copy_favicon(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(favicon) = resolved_website_favicon(meta, project) else {
        return Ok(());
    };
    let normalized = favicon.path;

    // An external favicon URL is served by whoever hosts it; there is
    // nothing local to copy, and treating it as a path would probe a
    // nonsense filename and warn about it. The `<link>` is still
    // emitted (see `WebsiteFaviconTransform`).
    if quarto_util::is_external_url(&normalized) {
        return Ok(());
    }

    let src = project.dir.join(&normalized);
    let exists = runtime.path_exists(&src, None).map_err(|e| {
        QuartoError::other(format!(
            "Failed to probe favicon source {}: {}",
            src.display(),
            e
        ))
    })?;
    if !exists {
        // Name the config the user actually wrote: a project relying
        // on the brand fallback has no `website.favicon` to look at.
        diagnostics.push(DiagnosticMessage::warning(format!(
            "{} refers to missing file '{}'",
            favicon.origin.describe(),
            normalized
        )));
        return Ok(());
    }

    copy_asset_file(project, runtime, &normalized, "favicon")
}

/// Copy the navbar logo (`website.navbar.logo` / `navbar.logo`) from
/// the project root to the output directory.
///
/// Decision 5 of bd-root-relative-paths-design-fc5pvkcv: favicon is
/// not special — config-declared assets q2 knows about get the same
/// warn-and-continue copy treatment. Same no-op cases as
/// [`copy_favicon`]: no metadata, no navbar/logo, external URL.
/// A leading `/` is site-root-relative (decision 4) and strips to the
/// same project-relative path.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn copy_navbar_logo(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(navbar) = quarto_navigation::resolve_navbar(meta) else {
        return Ok(());
    };
    let Some(logo) = navbar.logo else {
        return Ok(());
    };
    // Copy each distinct variant file (a single logo has identical
    // halves; copying the same path twice would be harmless but noisy).
    let mut paths: Vec<&str> = vec![&logo.light.path];
    if logo.dark.path != logo.light.path {
        paths.push(&logo.dark.path);
    }
    for raw in paths {
        // External logo URLs are served by whoever hosts them (mirrors
        // the favicon rule, and checks before slash-stripping so
        // protocol-relative `//host/x` is never misread as site-rooted).
        if quarto_util::is_external_url(raw) {
            continue;
        }
        let normalized = raw.strip_prefix('/').unwrap_or(raw);
        if normalized.is_empty() {
            continue;
        }

        let src = project.dir.join(normalized);
        let exists = runtime.path_exists(&src, None).map_err(|e| {
            QuartoError::other(format!(
                "Failed to probe navbar logo source {}: {}",
                src.display(),
                e
            ))
        })?;
        if !exists {
            diagnostics.push(DiagnosticMessage::warning(format!(
                "website.navbar.logo refers to missing file '{}'",
                normalized
            )));
            continue;
        }

        copy_asset_file(project, runtime, normalized, "navbar logo")?;
    }
    Ok(())
}

/// Copy images referenced from `page-footer` regions — Text regions
/// *and* item `text:` (bd-page-footer-image-items-stmpikgo, Phase 4)
/// — into the output tree.
///
/// Decision 5 of bd-root-relative-paths-design-fc5pvkcv, footer
/// edition: a footer-region markdown image (`![](/images/x.svg)`) is
/// a config-declared asset like the favicon and navbar logo. External
/// URLs and `data:` URIs are skipped; a leading `/` is
/// site-root-relative and strips to the project-relative path;
/// `?query`/`#fragment` tails are dropped for the file probe.
///
/// A missing file raises the same **`Q-5-6`** warning the identical
/// reference would raise in a document body, located at the reference
/// inside the config file (the CLI's project-diagnostic printer binds
/// `_quarto.yml` into the source context, so the span renders as an
/// Ariadne snippet). Running here — once per project, post-render —
/// rather than in the per-doc pipeline is what keeps a broken footer
/// reference from warning once per rendered page.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn copy_footer_images(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    use quarto_navigation::FooterRegion;

    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(footer) = quarto_navigation::resolve_page_footer(meta) else {
        return Ok(());
    };

    let mut refs: Vec<ImageRef> = Vec::new();
    for region in [&footer.left, &footer.center, &footer.right] {
        match region {
            FooterRegion::Text(cv) => collect_config_text_images(cv, &mut refs),
            FooterRegion::Items(items) => collect_items_images(items, &mut refs),
            FooterRegion::Empty => {}
        }
    }

    for ImageRef { url: raw, origin } in refs {
        if quarto_util::is_external_url(&raw) {
            continue;
        }
        let path = raw.split(['#', '?']).next().unwrap_or(raw.as_str());
        let normalized = path.strip_prefix('/').unwrap_or(path);
        if normalized.is_empty() {
            continue;
        }
        let src = project.dir.join(normalized);
        let exists = runtime.path_exists(&src, None).map_err(|e| {
            QuartoError::other(format!(
                "Failed to probe page-footer image source {}: {}",
                src.display(),
                e
            ))
        })?;
        if !exists {
            // Uniform missing-resource shape (Q-5-6): the same
            // intent-based diagnostic the body's resource-copy drain
            // emits, anchored at the reference in the YAML.
            let intent = crate::render::ResourceCopyIntent {
                src,
                dest: project.output_dir.join(normalized),
                origin,
            };
            diagnostics
                .push(crate::resource_copy_diagnostics::missing_resource_diagnostic(&intent));
            continue;
        }
        copy_asset_file(project, runtime, normalized, "page-footer image")?;
    }
    Ok(())
}

/// One collected image reference: the raw authored URL plus the span
/// to anchor a `Q-5-6` at — the URL's own span when the parse tracked
/// it, else the whole image's.
#[cfg(not(target_arch = "wasm32"))]
struct ImageRef {
    url: String,
    origin: quarto_source_map::SourceInfo,
}

/// Collect image references from a text-bearing config value in any
/// of its shapes: parsed inlines, parsed blocks, or a raw scalar.
///
/// At post-render time the project config still holds raw scalars —
/// markdown-izing config strings (`ConfigMarkdownTransform`) happens
/// in the per-doc pipeline — so scalars are re-parsed the same way
/// here; parse warnings are dropped, the per-doc pipeline already
/// reported them. The parse threads the scalar's `SourceInfo`
/// through, so collected spans remap into the config file.
#[cfg(not(target_arch = "wasm32"))]
fn collect_config_text_images(
    cv: &quarto_pandoc_types::config_value::ConfigValue,
    out: &mut Vec<ImageRef>,
) {
    use quarto_pandoc_types::config_value::ConfigValueKind;
    match &cv.value {
        ConfigValueKind::PandocInlines(inlines) => collect_inline_image_refs(inlines, out),
        ConfigValueKind::PandocBlocks(blocks) => collect_block_image_refs(blocks, out),
        ConfigValueKind::Scalar { .. } => {
            let Some(text) = cv.as_plain_text() else {
                return;
            };
            let mut parse_diags = Vec::new();
            let kind = pampa::pandoc::meta::parse_config_string_as_markdown(
                &text,
                &cv.source_info,
                &mut parse_diags,
            );
            match &kind {
                ConfigValueKind::PandocInlines(inlines) => collect_inline_image_refs(inlines, out),
                ConfigValueKind::PandocBlocks(blocks) => collect_block_image_refs(blocks, out),
                _ => {}
            }
        }
        _ => {}
    }
}

/// Collect image references from footer items: each item's `text:`
/// and (bare-scalar) `bare_text`, recursing into `menu` symmetrically
/// with the render-time walkers.
#[cfg(not(target_arch = "wasm32"))]
fn collect_items_images(items: &[quarto_navigation::NavigationItem], out: &mut Vec<ImageRef>) {
    for item in items {
        if let Some(cv) = &item.text {
            collect_config_text_images(cv, out);
        }
        if let Some(cv) = &item.bare_text {
            collect_config_text_images(cv, out);
        }
        collect_items_images(&item.menu, out);
    }
}

/// Collect image references from block containers. Coverage mirrors
/// `transforms::navigation_href::rewrite_config_blocks` for the
/// container shapes that plausibly occur in config text; figures
/// contribute the image in their content (and any images in their
/// caption).
#[cfg(not(target_arch = "wasm32"))]
fn collect_block_image_refs(blocks: &[quarto_pandoc_types::block::Block], out: &mut Vec<ImageRef>) {
    use quarto_pandoc_types::block::Block;
    for block in blocks {
        match block {
            Block::Plain(p) => collect_inline_image_refs(&p.content, out),
            Block::Paragraph(p) => collect_inline_image_refs(&p.content, out),
            Block::Header(h) => collect_inline_image_refs(&h.content, out),
            Block::LineBlock(lb) => {
                for line in &lb.content {
                    collect_inline_image_refs(line, out);
                }
            }
            Block::BlockQuote(bq) => collect_block_image_refs(&bq.content, out),
            Block::OrderedList(ol) => {
                for item in &ol.content {
                    collect_block_image_refs(item, out);
                }
            }
            Block::BulletList(bl) => {
                for item in &bl.content {
                    collect_block_image_refs(item, out);
                }
            }
            Block::DefinitionList(dl) => {
                for (term, defs) in &dl.content {
                    collect_inline_image_refs(term, out);
                    for def in defs {
                        collect_block_image_refs(def, out);
                    }
                }
            }
            Block::Div(d) => collect_block_image_refs(&d.content, out),
            Block::Figure(f) => {
                if let Some(short) = &f.caption.short {
                    collect_inline_image_refs(short, out);
                }
                if let Some(long) = &f.caption.long {
                    collect_block_image_refs(long, out);
                }
                collect_block_image_refs(&f.content, out);
            }
            _ => {}
        }
    }
}

/// Collect `Image` target references from config-region inlines, in
/// order, deduplicated by URL (the first occurrence keeps its span),
/// recursing through formatting containers. The span follows the body
/// collector's rule (`ResourceCollectorTransform`): the URL's own
/// span when the parse tracked it, else the whole image's.
#[cfg(not(target_arch = "wasm32"))]
fn collect_inline_image_refs(
    inlines: &[quarto_pandoc_types::inline::Inline],
    out: &mut Vec<ImageRef>,
) {
    use quarto_pandoc_types::inline::Inline;
    for inline in inlines {
        match inline {
            Inline::Image(img) => {
                if !out.iter().any(|r| r.url == img.target.0) {
                    out.push(ImageRef {
                        url: img.target.0.clone(),
                        origin: img
                            .target_source
                            .url
                            .clone()
                            .unwrap_or_else(|| img.source_info.clone()),
                    });
                }
                collect_inline_image_refs(&img.content, out);
            }
            Inline::Link(l) => collect_inline_image_refs(&l.content, out),
            Inline::Emph(e) => collect_inline_image_refs(&e.content, out),
            Inline::Strong(s) => collect_inline_image_refs(&s.content, out),
            Inline::Underline(u) => collect_inline_image_refs(&u.content, out),
            Inline::Strikeout(s) => collect_inline_image_refs(&s.content, out),
            Inline::Superscript(s) => collect_inline_image_refs(&s.content, out),
            Inline::Subscript(s) => collect_inline_image_refs(&s.content, out),
            Inline::SmallCaps(s) => collect_inline_image_refs(&s.content, out),
            Inline::Quoted(q) => collect_inline_image_refs(&q.content, out),
            Inline::Span(s) => collect_inline_image_refs(&s.content, out),
            Inline::Insert(i) => collect_inline_image_refs(&i.content, out),
            Inline::Delete(d) => collect_inline_image_refs(&d.content, out),
            Inline::Highlight(h) => collect_inline_image_refs(&h.content, out),
            _ => {}
        }
    }
}

/// Copy `<project>/<normalized>` → `<output>/<normalized>`, creating
/// parent directories. Shared tail of the config-asset copy hooks
/// ([`copy_favicon`], [`copy_navbar_logo`], [`copy_footer_images`]);
/// callers have already resolved, normalized, and existence-checked
/// the path.
#[cfg(not(target_arch = "wasm32"))]
fn copy_asset_file(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    normalized: &str,
    what: &str,
) -> Result<()> {
    let src = project.dir.join(normalized);
    let dst = project.output_dir.join(normalized);
    if let Some(parent) = dst.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            QuartoError::other(format!(
                "Failed to create {} directory {}: {}",
                what,
                parent.display(),
                e
            ))
        })?;
    }
    runtime.file_copy(&src, &dst).map_err(|e| {
        QuartoError::other(format!(
            "Failed to copy {} {} → {}: {}",
            what,
            src.display(),
            dst.display(),
            e
        ))
    })?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Sitemap (Phase 7)
// ═══════════════════════════════════════════════════════════════════

/// One entry of a sitemap urlset.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, PartialEq, Eq)]
struct SitemapEntry {
    /// The fully-qualified URL of the page (XML-escaped).
    loc: String,
    /// ISO-8601 UTC timestamp, second precision; `None` to omit.
    lastmod: Option<String>,
}

/// Emit `<output_dir>/sitemap.xml` listing every rendered page.
///
/// No-op when `website.site-url` is unset (Q1 parity).
///
/// **Phase 8 (`bd-pphv`):** the writer now does an
/// incremental merge instead of a fresh write:
///
/// 1. Read the existing `sitemap.xml`, parsing each `<url>` entry
///    into a `loc → lastmod` map.
/// 2. Walk `index.profiles()`. For each profile:
///    - If the page was rendered this run (its output path
///      appears in `outputs`), use the input file's current
///      mtime.
///    - Otherwise, look up the page's `loc` in the previous
///      sitemap. If found, preserve that `lastmod` (the page
///      hasn't been re-rendered, so its on-disk lastmod is
///      still authoritative). If not found (e.g. a brand-new
///      page that wasn't in the targets this run), fall back
///      to the current mtime.
/// 3. Pages no longer in the index are dropped.
/// 4. Write back, sorted by `loc` for stability.
///
/// If the existing sitemap can't be read or parsed, the merge
/// degrades to a fresh-write — same shape as Phase 7.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write_sitemap(
    project: &ProjectContext,
    index: &ProjectIndex,
    output_paths: &[std::path::PathBuf],
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(site_url) = website_site_url(meta) else {
        return Ok(());
    };
    let base = site_url.trim_end_matches('/');

    // Set of output paths (absolute) that were rendered this run.
    // Used to decide whether a profile's lastmod should refresh
    // (rendered) or preserve from the existing sitemap (skipped).
    let rendered_outputs: std::collections::HashSet<&Path> =
        output_paths.iter().map(|p| p.as_path()).collect();

    // Try to read the existing sitemap. Failure is non-fatal —
    // we fall back to fresh-write.
    let dst = project.output_dir.join("sitemap.xml");
    let prior: std::collections::HashMap<String, String> = runtime
        .file_read(&dst)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .map(|xml| parse_sitemap_locs(&xml))
        .unwrap_or_default();

    let mut entries: Vec<SitemapEntry> = Vec::with_capacity(index.profiles().len());
    for profile in index.profiles() {
        let loc_raw = format!("{}/{}", base, profile.output_href.trim_start_matches('/'));
        let loc_escaped = escape_xml_text(&loc_raw);

        // Was this page rendered this run? Check by comparing the
        // expected output path to the rendered set.
        let expected_output = project.output_dir.join(&profile.output_href);
        let was_rendered = rendered_outputs.contains(expected_output.as_path());

        let lastmod = if was_rendered {
            // Rendered this run → fresh mtime from the input file.
            let source_path = project.dir.join(&profile.source_path);
            read_input_mtime(&source_path, runtime)
        } else {
            // Skipped this run → preserve the previous sitemap's
            // lastmod when available. If the page is new (not in
            // the prior sitemap), fall back to the current mtime
            // so the entry still has a sensible date.
            prior.get(&loc_escaped).cloned().or_else(|| {
                let source_path = project.dir.join(&profile.source_path);
                read_input_mtime(&source_path, runtime)
            })
        };

        entries.push(SitemapEntry {
            loc: loc_escaped,
            lastmod,
        });
    }

    // Sort by loc for deterministic output. (Pre-Phase-8 fresh
    // writes used insertion order; sorting now makes the merge
    // stable across runs that may visit profiles in different
    // orders.)
    entries.sort_by(|a, b| a.loc.cmp(&b.loc));

    let xml = render_sitemap_xml(&entries);
    if let Some(parent) = dst.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            QuartoError::other(format!(
                "Failed to create sitemap directory {}: {}",
                parent.display(),
                e
            ))
        })?;
    }
    runtime.file_write(&dst, xml.as_bytes()).map_err(|e| {
        QuartoError::other(format!("Failed to write sitemap {}: {}", dst.display(), e))
    })?;
    Ok(())
}

/// Parse `loc → lastmod` out of an existing sitemap XML.
///
/// Tolerant scanner: skips malformed `<url>` blocks, returns an
/// empty map on root-level parse failures. We wrote this file
/// ourselves last run (with `render_sitemap_xml`) so we know its
/// shape; the parser is opinionated about that shape and falls
/// back gracefully when reality disagrees.
///
/// `loc` strings are returned in their XML-escaped form (the same
/// form the writer compares against), so callers can map directly
/// from the freshly-computed escaped loc to the prior `lastmod`.
#[cfg(not(target_arch = "wasm32"))]
fn parse_sitemap_locs(xml: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let mut cursor = 0;
    while let Some(start_rel) = xml[cursor..].find("<url>") {
        let block_start = cursor + start_rel + "<url>".len();
        let block_end_rel = match xml[block_start..].find("</url>") {
            Some(i) => i,
            None => break,
        };
        let block = &xml[block_start..block_start + block_end_rel];
        cursor = block_start + block_end_rel + "</url>".len();

        let loc = match extract_inner_tag(block, "loc") {
            Some(s) => s,
            None => continue,
        };
        let lastmod = extract_inner_tag(block, "lastmod");
        if let Some(lm) = lastmod {
            out.insert(loc.to_string(), lm.to_string());
        }
        // Entries without a `<lastmod>` are not retained — there's
        // nothing to preserve. The writer will compute a fresh
        // lastmod for them anyway.
    }
    out
}

/// Extract the text between `<tag>` and `</tag>` in `block`.
/// Returns `None` if either tag is missing. Doesn't unescape the
/// XML entities — the caller compares against escaped strings.
#[cfg(not(target_arch = "wasm32"))]
fn extract_inner_tag<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end_rel = block[start..].find(&close)?;
    Some(&block[start..start + end_rel])
}

/// Read an input file's mtime as an ISO-8601 UTC string with
/// second precision. Returns `None` if the metadata is unreadable
/// or has no mtime.
#[cfg(not(target_arch = "wasm32"))]
fn read_input_mtime(path: &Path, runtime: &dyn SystemRuntime) -> Option<String> {
    let metadata = runtime.path_metadata(path).ok()?;
    metadata.modified.map(format_iso8601_utc)
}

/// Format a [`SystemTime`] as `YYYY-MM-DDThh:mm:ssZ` in UTC.
///
/// Manual algorithm so we don't pull in `chrono`. Computes
/// civil-date components from the POSIX seconds-since-epoch using
/// the Howard Hinnant calendar formulas (proleptic Gregorian).
#[cfg(not(target_arch = "wasm32"))]
fn format_iso8601_utc(time: SystemTime) -> String {
    let secs = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };

    let days_since_epoch = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);

    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = (secs_of_day / 3600) as u32;
    let minute = ((secs_of_day % 3600) / 60) as u32;
    let second = (secs_of_day % 60) as u32;

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

/// Civil-date conversion (Howard Hinnant, public domain).
///
/// `days` is days since 1970-01-01. Returns (year, month, day) in
/// the proleptic Gregorian calendar.
#[cfg(not(target_arch = "wasm32"))]
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146_096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let y = y + if m <= 2 { 1 } else { 0 };
    (y as i32, m, d)
}

/// XML-text escaper for `<loc>` content.
///
/// Escapes `&`, `<`, `>`, `"`, `'`. Keeps the helper inline so we
/// don't pull in an XML library for ~5 characters of substitution.
#[cfg(not(target_arch = "wasm32"))]
fn escape_xml_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Hand-rolled sitemap XML emitter.
///
/// Format matches Q1's
/// `external-sources/quarto-cli/src/resources/projects/website/templates/sitemap.ejs.xml`.
#[cfg(not(target_arch = "wasm32"))]
fn render_sitemap_xml(entries: &[SitemapEntry]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n");
    for entry in entries {
        out.push_str("  <url>\n");
        out.push_str("    <loc>");
        out.push_str(&entry.loc);
        out.push_str("</loc>\n");
        if let Some(lastmod) = &entry.lastmod {
            out.push_str("    <lastmod>");
            out.push_str(lastmod);
            out.push_str("</lastmod>\n");
        }
        out.push_str("  </url>\n");
    }
    out.push_str("</urlset>\n");
    out
}

// ═══════════════════════════════════════════════════════════════════
// robots.txt (Phase 7)
// ═══════════════════════════════════════════════════════════════════

/// Emit `<output_dir>/robots.txt`.
///
/// Behavior:
/// 1. If `<project>/robots.txt` exists → copy it verbatim.
/// 2. Else if `website.site-url` is set → write
///    `Sitemap: <site-url>/sitemap.xml\n`.
/// 3. Else → no-op.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write_robots_txt(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    let dst = project.output_dir.join("robots.txt");
    let src = project.dir.join("robots.txt");

    let user_robots_exists = runtime.path_exists(&src, None).map_err(|e| {
        QuartoError::other(format!(
            "Failed to probe robots.txt source {}: {}",
            src.display(),
            e
        ))
    })?;

    if user_robots_exists {
        if let Some(parent) = dst.parent() {
            runtime.dir_create(parent, true).map_err(|e| {
                QuartoError::other(format!(
                    "Failed to create output directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        runtime.file_copy(&src, &dst).map_err(|e| {
            QuartoError::other(format!(
                "Failed to copy robots.txt {} → {}: {}",
                src.display(),
                dst.display(),
                e
            ))
        })?;
        return Ok(());
    }

    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(site_url) = website_site_url(meta) else {
        return Ok(());
    };
    let base = site_url.trim_end_matches('/');
    let body = format!("Sitemap: {base}/sitemap.xml\n");

    if let Some(parent) = dst.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            QuartoError::other(format!(
                "Failed to create robots.txt directory {}: {}",
                parent.display(),
                e
            ))
        })?;
    }
    runtime.file_write(&dst, body.as_bytes()).map_err(|e| {
        QuartoError::other(format!(
            "Failed to write robots.txt {}: {}",
            dst.display(),
            e
        ))
    })?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Alias redirect stubs — native-only
// ═══════════════════════════════════════════════════════════════════

/// Write a redirect stub for every `aliases:` entry in the project.
///
/// Planning is pure and lives in [`crate::project::aliases`]; this
/// function is the part that touches disk. It writes nothing at all
/// when the plan reports a conflict — a half-written set of redirects
/// is worse than none, and by Phase 1's contract a hook failure aborts
/// the render.
///
/// Unlike the other hooks here this one is not gated on a config key:
/// `aliases:` is per-document, so the plan is simply empty when no
/// page declares any.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn write_alias_redirects(
    project: &ProjectContext,
    index: &ProjectIndex,
    runtime: &dyn SystemRuntime,
) -> Result<()> {
    use crate::project::aliases::{plan_alias_stubs, render_stub};

    let plan = plan_alias_stubs(index.profiles());

    if !plan.conflicts.is_empty() {
        return Err(QuartoError::Parse(alias_conflicts_to_parse_error(
            &plan.conflicts,
            project,
            runtime,
        )));
    }

    for stub in &plan.stubs {
        let dst = project.output_dir.join(&stub.stub_href);
        if let Some(parent) = dst.parent() {
            runtime.dir_create(parent, true).map_err(|e| {
                QuartoError::other(format!(
                    "Failed to create alias directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        runtime
            .file_write(&dst, render_stub(stub).as_bytes())
            .map_err(|e| {
                QuartoError::other(format!(
                    "Failed to write alias redirect {}: {}",
                    dst.display(),
                    e
                ))
            })?;
    }

    Ok(())
}

/// Turn every conflict into a diagnostic, sharing one `SourceContext`.
///
/// All of them, not the first: a site with dozens of aliasing files
/// should learn about its mistakes in one render rather than one per
/// render.
#[cfg(not(target_arch = "wasm32"))]
fn alias_conflicts_to_parse_error(
    conflicts: &[crate::project::aliases::AliasConflict],
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
) -> crate::error::ParseError {
    use quarto_source_map::SourceContext;

    let mut source_context = SourceContext::new();
    let diagnostics = conflicts
        .iter()
        .map(|conflict| alias_conflict_diagnostic(conflict, project, runtime, &mut source_context))
        .collect();
    crate::error::ParseError::new(diagnostics, source_context)
}

/// Register the file an alias was written in and return its span,
/// re-keyed so several documents can share one `SourceContext`.
///
/// An alias can be declared in the page's own front matter (spans
/// rooted at the document parse context's dense `FileId(0)`) or
/// inherited from a directory `_metadata.yml` or `_quarto.yml`
/// (quarto-yaml filename-hash ids). Both id schemes go into the
/// candidate list, and
/// [`rebase_source_candidates`](crate::config_sources::rebase_source_candidates)
/// picks the one whose id actually matches — never binding a
/// non-match, so a span is either right or absent.
///
/// The *rebasing* matters here specifically because a collision
/// diagnostic names two pages: without it, the second document's
/// `FileId(0)` offsets would be rendered against the first
/// document's text.
#[cfg(not(target_arch = "wasm32"))]
fn locate_alias(
    who: &crate::project::aliases::AliasRef,
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    source_context: &mut quarto_source_map::SourceContext,
) -> Option<quarto_source_map::SourceInfo> {
    use quarto_source_map::FileId;

    let doc_source = project.dir.join(&who.source_path);
    let config = &project.config;
    let layer_paths =
        crate::project::directory_metadata_paths_for_document(project, &doc_source, runtime);
    let hash = |p: &Path| quarto_yaml::file_id_for_filename(&p.to_string_lossy());

    let candidates = std::iter::once((FileId(0), doc_source.as_path()))
        .chain(config.config_path.as_deref().map(|p| (hash(p), p)))
        .chain(
            config
                .profile_config_paths
                .iter()
                .map(|p| (hash(p), p.as_path())),
        )
        .chain(layer_paths.iter().map(|p| (hash(p), p.as_path())));

    crate::config_sources::rebase_source_candidates(source_context, &who.source_info, candidates)
        .map(|(_, span)| span)
}

/// Render one conflict as a diagnostic.
#[cfg(not(target_arch = "wasm32"))]
fn alias_conflict_diagnostic(
    conflict: &crate::project::aliases::AliasConflict,
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    source_context: &mut quarto_source_map::SourceContext,
) -> DiagnosticMessage {
    use crate::project::aliases::AliasConflict;
    use quarto_error_reporting::DiagnosticMessageBuilder;

    // `with_location` takes an owned SourceInfo and there is no
    // `with_optional_location`, so each arm threads the Option.
    let at = |builder: DiagnosticMessageBuilder,
              span: Option<quarto_source_map::SourceInfo>|
     -> DiagnosticMessageBuilder {
        match span {
            Some(info) => builder.with_location(info),
            None => builder,
        }
    };

    match conflict {
        AliasConflict::OverwritesPage {
            alias,
            stub_href,
            page_source,
        } => {
            let span = locate_alias(alias, project, runtime, source_context);
            at(
                DiagnosticMessageBuilder::error("Alias would overwrite a rendered page")
                    .with_code("Q-5-23"),
                span,
            )
            .problem(format!(
                "`{}` in `{}` resolves to `{}`, which is where `{}` renders. \
                 Only one file can exist there.",
                alias.alias,
                alias.source_path.display(),
                stub_href,
                page_source.display()
            ))
            .add_info(
                "Quarto 1 skips the redirect with a warning. Quarto 2 stops instead: a site \
                 that builds while silently missing a redirect keeps 404ing old links with \
                 nothing in the output to say why.",
            )
            .add_hint("Point the alias at a path no page renders to?")
            .build()
        }

        AliasConflict::DuplicateClaim {
            first,
            second,
            stub_href,
            fragment,
        } => {
            let first_span = locate_alias(first, project, runtime, source_context);
            let second_span = locate_alias(second, project, runtime, source_context);
            let route = if fragment.is_empty() {
                format!("`{stub_href}`")
            } else {
                format!("`{stub_href}#{fragment}`")
            };
            let builder = at(
                DiagnosticMessageBuilder::error("Two pages claim the same alias")
                    .with_code("Q-5-24"),
                first_span,
            )
            .problem(format!(
                "`{}` and `{}` both redirect {route} to themselves. A redirect can only \
                 send visitors to one page.",
                first.source_path.display(),
                second.source_path.display()
            ));
            let builder = match second_span {
                Some(info) => builder.add_info_at(
                    format!("Also claimed by `{}` here.", second.source_path.display()),
                    info,
                ),
                None => builder.add_info(format!(
                    "Also claimed by `{}` (`{}`).",
                    second.source_path.display(),
                    second.alias
                )),
            };
            builder
                .add_hint("Remove the alias from one of the two pages?")
                .build()
        }

        AliasConflict::NoDefaultOwner {
            stub_href,
            contributors,
        } => {
            let span = contributors
                .first()
                .and_then(|who| locate_alias(who, project, runtime, source_context));
            let names = contributors
                .iter()
                .map(|c| format!("`{}`", c.source_path.display()))
                .collect::<Vec<_>>()
                .join(", ");
            at(
                DiagnosticMessageBuilder::error("No page owns the alias's fragment-less URL")
                    .with_code("Q-5-24"),
                span,
            )
            .problem(format!(
                "{names} route fragments through `{stub_href}`, but no page claims it \
                 without a fragment, so a visitor arriving at the bare URL has no \
                 destination."
            ))
            .add_info(
                "Quarto 1 sends that visitor to the site root. Picking one of these pages \
                 instead would be a guess about which one you meant.",
            )
            .add_hint("Add the fragment-less alias to whichever page should own the old URL?")
            .build()
        }

        AliasConflict::CaseOnlyAliasCollision { first, second } => {
            let first_span = locate_alias(first, project, runtime, source_context);
            let second_span = locate_alias(second, project, runtime, source_context);
            let builder = at(
                DiagnosticMessageBuilder::error("Aliases differ only by case").with_code("Q-5-25"),
                first_span,
            )
            .problem(format!(
                "`{}` in `{}` and `{}` in `{}` resolve to paths that differ only in \
                 capitalization. macOS and Windows treat those as one file.",
                first.alias,
                first.source_path.display(),
                second.alias,
                second.source_path.display()
            ))
            .add_info(
                "Checked on every platform, including case-sensitive ones, so a Linux build \
                 cannot ship a site that loses a redirect when served from macOS or Windows.",
            );
            let builder = match second_span {
                Some(info) => builder.add_info_at("The other spelling is here.", info),
                None => builder,
            };
            builder
                .add_hint("Rename one so the two differ by more than capitalization?")
                .build()
        }

        AliasConflict::CaseOnlyPageCollision {
            alias,
            stub_href,
            page_href,
            page_source,
        } => {
            let span = locate_alias(alias, project, runtime, source_context);
            at(
                DiagnosticMessageBuilder::error("Alias differs only by case from a rendered page")
                    .with_code("Q-5-25"),
                span,
            )
            .problem(format!(
                "`{}` in `{}` resolves to `{}`, which differs only in capitalization from \
                 `{}` rendered by `{}`. macOS and Windows treat those as one file.",
                alias.alias,
                alias.source_path.display(),
                stub_href,
                page_href,
                page_source.display()
            ))
            .add_hint("Rename the alias so it cannot collide with the page?")
            .build()
        }

        AliasConflict::EscapesOutputDir { alias } => {
            let span = locate_alias(alias, project, runtime, source_context);
            at(
                DiagnosticMessageBuilder::error("Alias resolves outside the output directory")
                    .with_code("Q-5-26"),
                span,
            )
            .problem(format!(
                "`{}` in `{}` climbs above the site's output directory. Redirect stubs must \
                 be written inside the site.",
                alias.alias,
                alias.source_path.display()
            ))
            .add_info(
                "A relative alias resolves against the page's own output location, not the \
                 project root, so a page deep in the tree can climb further than expected.",
            )
            .add_hint("Use a site-root-relative alias such as `/old.html`?")
            .build()
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use std::time::Duration;

    /// Phase 9 §Decision 4 invariant: for every artifact, the URL
    /// embedded in HTML by `html_url_for(Project, p)` and the
    /// write target computed by `on_disk_path_for(Project, p)`
    /// must round-trip through the *same* resolver.
    ///
    /// On VFS-root mode the html_url is absolute (`/<vfs_root>/<p>`)
    /// and the on-disk path is the same with the leading `/`
    /// dropped (since paths are absolute already). The browser
    /// fetches the URL and the hub-client serves from VFS at the
    /// matching synthetic path. This test pins that contract:
    /// regression-proofs against a future patch that changes one
    /// computation but not the other.
    #[test]
    fn vfs_root_resolver_url_matches_on_disk_path() {
        use crate::artifact::ArtifactScope;
        use crate::resource_resolver::ResourceResolverContext;

        let resolver = ResourceResolverContext::vfs_root("/.quarto/project-artifacts");
        let p = std::path::Path::new("quarto/theme.css");

        let url = resolver.html_url_for(ArtifactScope::Project, p);
        let on_disk = resolver.on_disk_path_for(ArtifactScope::Project, p);
        let on_disk_str = on_disk.to_string_lossy().replace('\\', "/");

        assert_eq!(
            url, on_disk_str,
            "html URL and on-disk path must match under vfs_root mode"
        );
    }

    fn entry_loc(loc: &str, lastmod: Option<&str>) -> SitemapEntry {
        SitemapEntry {
            loc: loc.to_string(),
            lastmod: lastmod.map(String::from),
        }
    }

    /// Plan test 23: zero entries → conformant empty urlset.
    #[test]
    fn sitemap_xml_empty_urlset() {
        let xml = render_sitemap_xml(&[]);
        assert_eq!(
            xml,
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">\n\
             </urlset>\n"
        );
    }

    /// Plan test 24: single entry → conformant XML with `<loc>` and
    /// `<lastmod>`.
    #[test]
    fn sitemap_xml_single_entry() {
        let xml = render_sitemap_xml(&[entry_loc(
            "https://example.com/index.html",
            Some("2026-04-27T14:32:11Z"),
        )]);
        assert!(
            xml.contains("<loc>https://example.com/index.html</loc>"),
            "missing loc: {xml}"
        );
        assert!(
            xml.contains("<lastmod>2026-04-27T14:32:11Z</lastmod>"),
            "missing lastmod: {xml}"
        );
        assert!(
            xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
            "missing prologue: {xml}"
        );
    }

    /// Plan test 25: special characters in `loc` are XML-escaped
    /// before they hit the output (entry is built with the
    /// already-escaped string from `escape_xml_text`).
    #[test]
    fn sitemap_xml_escapes_special_chars() {
        let escaped = escape_xml_text("https://example.com/?q=a&b=c");
        assert_eq!(escaped, "https://example.com/?q=a&amp;b=c");
        let xml = render_sitemap_xml(&[entry_loc(&escaped, None)]);
        assert!(xml.contains("<loc>https://example.com/?q=a&amp;b=c</loc>"));
    }

    /// Plan test 26: an entry with no mtime omits `<lastmod>`.
    #[test]
    fn sitemap_xml_omits_lastmod_when_unknown() {
        let xml = render_sitemap_xml(&[entry_loc("https://example.com/x.html", None)]);
        assert!(!xml.contains("<lastmod>"), "unexpected lastmod: {xml}");
        assert!(xml.contains("<loc>https://example.com/x.html</loc>"));
    }

    /// Plan test 27 reframed: site-url with trailing slash + simple
    /// output_href compose with one `/` separator (the actual
    /// composition lives in `write_sitemap`; verify the helper
    /// behavior here).
    #[test]
    fn sitemap_url_join_strips_trailing_slash() {
        let base = "https://example.com/".trim_end_matches('/');
        let href = "x.html".trim_start_matches('/');
        assert_eq!(format!("{base}/{href}"), "https://example.com/x.html");
    }

    /// Plan test 28: default robots.txt body for a known site-url.
    #[test]
    fn robots_txt_default_body() {
        let base = "https://example.com".trim_end_matches('/');
        assert_eq!(
            format!("Sitemap: {base}/sitemap.xml\n"),
            "Sitemap: https://example.com/sitemap.xml\n"
        );
    }

    /// Plan test 29: trailing slash on site-url is stripped before
    /// the robots.txt body is composed.
    #[test]
    fn robots_txt_strips_trailing_slash_in_sitemap_url() {
        let base = "https://example.com/".trim_end_matches('/');
        assert_eq!(
            format!("Sitemap: {base}/sitemap.xml\n"),
            "Sitemap: https://example.com/sitemap.xml\n"
        );
    }

    /// XML escape covers all five special characters.
    #[test]
    fn xml_escape_table() {
        assert_eq!(escape_xml_text("&"), "&amp;");
        assert_eq!(escape_xml_text("<"), "&lt;");
        assert_eq!(escape_xml_text(">"), "&gt;");
        assert_eq!(escape_xml_text("\""), "&quot;");
        assert_eq!(escape_xml_text("'"), "&apos;");
        assert_eq!(escape_xml_text("plain text"), "plain text");
        assert_eq!(escape_xml_text("a & b"), "a &amp; b");
    }

    /// ISO-8601 formatter: epoch is `1970-01-01T00:00:00Z`.
    #[test]
    fn iso8601_unix_epoch() {
        assert_eq!(
            format_iso8601_utc(SystemTime::UNIX_EPOCH),
            "1970-01-01T00:00:00Z"
        );
    }

    /// ISO-8601 formatter: a known timestamp computes correctly.
    /// 2026-04-27T14:32:11Z = 1_777_300_331 seconds since the
    /// Unix epoch (computed by hand, cross-checked against the
    /// formatter).
    #[test]
    fn iso8601_known_timestamp() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_777_300_331);
        assert_eq!(format_iso8601_utc(t), "2026-04-27T14:32:11Z");
    }

    /// ISO-8601 formatter: an end-of-year timestamp.
    /// 2025-12-31T23:59:59Z = 1_767_225_599 seconds since epoch.
    #[test]
    fn iso8601_end_of_year() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_225_599);
        assert_eq!(format_iso8601_utc(t), "2025-12-31T23:59:59Z");
    }

    /// ISO-8601 formatter: a leap-day timestamp.
    /// 2024-02-29T12:00:00Z = 1_709_208_000 seconds since epoch.
    #[test]
    fn iso8601_leap_day() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_709_208_000);
        assert_eq!(format_iso8601_utc(t), "2024-02-29T12:00:00Z");
    }

    // === Phase 8 sub-phase 8.3: sitemap merge parser =====================

    /// `parse_sitemap_locs` extracts every `<url>` block's
    /// `<loc>` and `<lastmod>` into a map. Round-trip from a
    /// freshly-rendered sitemap.
    #[test]
    fn parse_sitemap_locs_round_trip() {
        let xml = render_sitemap_xml(&[
            SitemapEntry {
                loc: "https://example.com/index.html".into(),
                lastmod: Some("2026-04-27T10:00:00Z".into()),
            },
            SitemapEntry {
                loc: "https://example.com/about.html".into(),
                lastmod: Some("2026-04-27T11:00:00Z".into()),
            },
        ]);
        let parsed = parse_sitemap_locs(&xml);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed
                .get("https://example.com/index.html")
                .map(String::as_str),
            Some("2026-04-27T10:00:00Z")
        );
        assert_eq!(
            parsed
                .get("https://example.com/about.html")
                .map(String::as_str),
            Some("2026-04-27T11:00:00Z")
        );
    }

    /// Entries lacking `<lastmod>` are not retained — there's
    /// nothing to preserve, and the writer falls back to the
    /// current mtime for them.
    #[test]
    fn parse_sitemap_locs_skips_entries_without_lastmod() {
        let xml = render_sitemap_xml(&[
            SitemapEntry {
                loc: "https://example.com/with.html".into(),
                lastmod: Some("2026-04-27T10:00:00Z".into()),
            },
            SitemapEntry {
                loc: "https://example.com/without.html".into(),
                lastmod: None,
            },
        ]);
        let parsed = parse_sitemap_locs(&xml);
        assert_eq!(parsed.len(), 1);
        assert!(parsed.contains_key("https://example.com/with.html"));
        assert!(!parsed.contains_key("https://example.com/without.html"));
    }

    /// Malformed input returns an empty map (or a partial one for
    /// the `<url>` blocks that did parse). Tolerant by design.
    #[test]
    fn parse_sitemap_locs_handles_garbage() {
        let parsed = parse_sitemap_locs("totally not xml");
        assert!(parsed.is_empty());

        // A `<url>` block missing `</url>` is silently skipped.
        let truncated = "<url><loc>https://x/y</loc>";
        let parsed = parse_sitemap_locs(truncated);
        assert!(parsed.is_empty());
    }

    /// XML-escaped locs round-trip without unescaping. The writer
    /// stores escaped locs in the `loc` field of `SitemapEntry`,
    /// so the lookup map keys on escaped strings.
    #[test]
    fn parse_sitemap_locs_keeps_loc_escaped() {
        let xml = render_sitemap_xml(&[SitemapEntry {
            loc: "https://example.com/a&amp;b.html".into(),
            lastmod: Some("2026-04-27T10:00:00Z".into()),
        }]);
        let parsed = parse_sitemap_locs(&xml);
        // The `&amp;` stays escaped in the parsed key — the
        // writer compares against the same escaped form.
        assert!(parsed.contains_key("https://example.com/a&amp;b.html"));
    }

    #[test]
    fn extract_inner_tag_simple() {
        assert_eq!(
            extract_inner_tag("<loc>https://example.com/x</loc>", "loc"),
            Some("https://example.com/x")
        );
    }

    #[test]
    fn extract_inner_tag_missing_returns_none() {
        assert_eq!(extract_inner_tag("<other>thing</other>", "loc"), None);
    }
}
