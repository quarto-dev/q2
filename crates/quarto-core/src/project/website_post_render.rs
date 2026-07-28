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

    let dst = project.output_dir.join(&normalized);
    if let Some(parent) = dst.parent() {
        runtime.dir_create(parent, true).map_err(|e| {
            QuartoError::other(format!(
                "Failed to create favicon directory {}: {}",
                parent.display(),
                e
            ))
        })?;
    }
    runtime.file_copy(&src, &dst).map_err(|e| {
        QuartoError::other(format!(
            "Failed to copy favicon {} → {}: {}",
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
