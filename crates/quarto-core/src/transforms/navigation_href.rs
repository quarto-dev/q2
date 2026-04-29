/*
 * navigation_href.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Href resolution helpers shared by navbar / sidebar / page-footer
//! Render transforms.
//!
//! A Render transform's job is to take the format-agnostic
//! `navigation.*` subtree (populated by the corresponding Generate
//! transform) and emit HTML. Entry hrefs in that subtree are still
//! project-relative source paths (e.g. `about.qmd`); the Render step
//! is where they become `about.html` via lookups in the
//! [`ProjectIndex`].
//!
//! Factored out of `sidebar_render.rs` during Phase 3 of the websites
//! epic so navbar + page-footer can share the same resolution rule and
//! diagnostic shape. See
//! `claude-notes/plans/2026-04-24-websites-phase-3.md` §Decision 3.

use std::path::{Path, PathBuf};

use quarto_error_reporting::DiagnosticMessage;

use crate::project::index::ProjectIndex;
use crate::resource_resolver::ResourceResolverContext;

/// Resolve an href for HTML output.
///
/// - External URLs (`http:`, `https:`, `mailto:`, `tel:`, `ftp:`,
///   `//host/…`) and fragment-only anchors (`#section`) pass through
///   unchanged.
/// - Project-relative source paths are rewritten to the referenced
///   document's output href via [`ProjectIndex::lookup_by_source`].
///   When `resolver` is attached the result is page-relative (via
///   [`ResourceResolverContext::page_url_for`]); without one, the
///   bare `output_href` is returned (defensive fallback for unit
///   tests / out-of-band callers — production callers always pass
///   a resolver). Query strings and fragments are preserved across
///   the rewrite.
/// - Source-path-shaped misses (looks like `*.qmd` but no matching
///   profile) emit a warning diagnostic naming the `source_label` and
///   the missing target. The raw href is preserved in the output so
///   the dangling link is at least visible to the reader.
///
/// `source_label` is a human-readable tag (`"Sidebar 'docs'"`,
/// `"Navbar"`, `"Page footer"`) — it appears verbatim in the
/// diagnostic so the user can locate the offending config.
///
/// **Symmetry with body links.** Phase 6's
/// [`resolve_doc_relative_href`] handles body-content links the
/// same way: lookup-then-relativize-via-resolver. The two helpers
/// differ only in input normalization — body links arrive
/// source-doc-relative and require [`resolve_to_project_root`]
/// first; nav hrefs arrive already project-root-relative (Phase 2
/// Decision 7/8). bd-swpy threaded the resolver here in 2026-04-29
/// to fix nav links 404-ing from pages in subdirectories.
pub fn resolve_href_for_html(
    raw: &str,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    source_label: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    // Split off any `?query` or `#fragment` tail so we look up just the
    // path portion. Preserve and re-append afterwards.
    let (path_part, tail) = match raw.find(|c| c == '#' || c == '?') {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };

    if let Some(idx) = index {
        if let Some(profile) = idx.lookup_by_source(Path::new(path_part)) {
            let url = match resolver {
                Some(r) => r.page_url_for(&profile.output_href),
                // No resolver: fall back to the bare project-relative
                // output href. Production callers always pass a
                // resolver; this branch is defensive (unit tests /
                // out-of-band callers may construct a RenderContext
                // without one).
                None => profile.output_href.clone(),
            };
            return format!("{}{}", url, tail);
        }
        // An index is present but the path didn't resolve — the user
        // has a project context and this looks like an intended
        // internal link, so surface it.
        if path_part.ends_with(".qmd") {
            let tag = source_label.unwrap_or("Navigation").to_string();
            diagnostics.push(DiagnosticMessage::warning(format!(
                "{} references unknown document '{}'",
                tag, path_part
            )));
        }
    }
    // Without an index (standalone single-doc render) we can't tell
    // whether a `.qmd` href is broken or intended-as-literal. Skip the
    // diagnostic to keep revealjs/etc. quiet; see Phase 3 plan Decision
    // 1 / tests 36 + 44.

    raw.to_string()
}

/// Classify an href as external (not project-relative).
///
/// Matches Quarto 1's cheap heuristic: anything with a recognizable
/// scheme, plus protocol-relative `//host/…` which bypasses the
/// project entirely.
pub fn is_external(href: &str) -> bool {
    href.starts_with("http://")
        || href.starts_with("https://")
        || href.starts_with("mailto:")
        || href.starts_with("tel:")
        || href.starts_with("ftp://")
        || href.starts_with("//")
}

/// Pass-1 / static counterpart to [`resolve_doc_relative_href`].
///
/// Returns the project-relative source path that an internal
/// `.qmd` reference resolves to (after `..` / `.` / leading-`/`
/// normalization), regardless of whether the target actually
/// exists in the project. Returns `None` for external URLs,
/// fragment-only anchors, and non-`.qmd` targets.
///
/// **Why no index parameter?** Pass-1 (Phase 8 sub-phase 8.0d's
/// `LinkResolutionStage`) runs *before* the project's
/// `ProjectIndex` is built — the index is constructed *from* the
/// profiles Pass-1 produces. Conditioning resolution on an index
/// lookup at that point would always fail (no index yet), so we
/// return the resolved path regardless. The Phase-8 dependency
/// graph builder filters body-link edges against the actual
/// index when it builds the graph, so unresolvable targets become
/// no-op edges. Phase 6's [`resolve_doc_relative_href`] (Pass-2)
/// keeps its index-aware lookup for diagnostics + rewriting.
///
/// Side-effect-free — no diagnostics, no resolver calls.
///
/// See `claude-notes/designs/body-link-resolution-contract.md` for
/// the prose contract.
pub fn resolve_doc_relative_target(raw: &str, source_relative: &str) -> Option<PathBuf> {
    if is_external(raw) || raw.starts_with('#') {
        return None;
    }
    let path_part = match raw.find(|c| c == '#' || c == '?') {
        Some(i) => &raw[..i],
        None => raw,
    };
    if !path_part.ends_with(".qmd") {
        // Non-.qmd hrefs are static resources, not project documents.
        // Match the diagnostic gating in resolve_doc_relative_href.
        return None;
    }
    let project_relative = resolve_to_project_root(source_relative, path_part);
    Some(PathBuf::from(project_relative))
}

/// Resolve a body-content href to a relative URL.
///
/// Companion to [`resolve_href_for_html`] for **body** content
/// (the inline `Link` nodes parsed from markdown). The two helpers
/// differ only in input normalization; both produce page-relative
/// URLs when a resolver is attached:
///
/// | Helper | Input | Output (with resolver) |
/// |--------|-------|------------------------|
/// | `resolve_href_for_html` | project-root-relative | page-relative |
/// | `resolve_doc_relative_href` | source-doc-relative | page-relative |
///
/// Phase 6 of the website-projects epic. See
/// `claude-notes/plans/2026-04-24-websites-phase-6.md` Decisions 3,
/// 4, 9, 10. (The nav helper picked up its resolver argument later
/// — see bd-swpy /
/// `claude-notes/plans/2026-04-29-bd-swpy-nav-href-relativization.md`.)
///
/// Algorithm:
/// 1. External URLs and fragment-only anchors pass through.
/// 2. Split off any `?query` or `#fragment` tail; keep for re-append.
/// 3. Resolve `path_part` against `dirname(source_relative)` using
///    [`resolve_to_project_root`] (handles leading `/`, `..`, `.`).
/// 4. Look up the project-relative path in `index`. If found, ask
///    `resolver` to compute the page-relative URL to the target's
///    `output_href`. Re-append the tail.
/// 5. Miss + `.qmd` shape + index present → emit a warning
///    diagnostic naming `source_label` and the missing path; return
///    the raw href verbatim so the dangling link is visible.
/// 6. No index → return the raw href verbatim (standalone render).
/// 7. No resolver → fall back to the bare `output_href` from the
///    profile (no relative-depth math). Defensive — production
///    callers always pass a resolver; only out-of-band callers
///    might not.
pub fn resolve_doc_relative_href(
    raw: &str,
    source_relative: &str,
    resolver: Option<&ResourceResolverContext>,
    index: Option<&ProjectIndex>,
    source_label: Option<&str>,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> String {
    if is_external(raw) || raw.starts_with('#') {
        return raw.to_string();
    }
    // Split off `?query` or `#fragment` tail; the lookup operates
    // on the path portion only, and we re-append the tail at the end.
    let (path_part, tail) = match raw.find(|c| c == '#' || c == '?') {
        Some(i) => (&raw[..i], &raw[i..]),
        None => (raw, ""),
    };

    // No index = standalone render; pass the href through verbatim.
    let Some(idx) = index else {
        return raw.to_string();
    };

    // Resolve doc-relative input to a project-root-relative path.
    let project_relative = resolve_to_project_root(source_relative, path_part);

    if let Some(profile) = idx.lookup_by_source(Path::new(&project_relative)) {
        let url = match resolver {
            Some(r) => r.page_url_for(&profile.output_href),
            // No resolver: fall back to the bare project-relative
            // output href. Production callers always pass a
            // resolver, so this branch is defensive.
            None => profile.output_href.clone(),
        };
        return format!("{}{}", url, tail);
    }

    // Miss. Surface a warning iff the target *looks like* a
    // renderable document (matches the `.qmd`-only convention from
    // `resolve_href_for_html`). Non-qmd misses pass through silent
    // since they may legitimately be static resources.
    if path_part.ends_with(".qmd") {
        let tag = source_label.unwrap_or("Body link").to_string();
        diagnostics.push(DiagnosticMessage::warning(format!(
            "{} references unknown document '{}'",
            tag, project_relative
        )));
    }

    raw.to_string()
}

/// Join a doc-relative or absolute link href against the source
/// document's directory and normalize `.` / `..` components,
/// producing a project-root-relative forward-slash path.
///
/// - `link_href` starting with `/` strips the leading slash and
///   ignores `source_relative`.
/// - Otherwise joins with `dirname(source_relative)`.
/// - `.` components are dropped; `..` pops the most recent
///   component, with extras above the root clamped (no error).
///
/// Forward-slash on input and output. Used by
/// [`resolve_doc_relative_href`].
fn resolve_to_project_root(source_relative: &str, link_href: &str) -> String {
    // Treat leading-`/` as project-root-absolute. Q1 parity.
    let (base_dir, rest): (&str, &str) = if let Some(stripped) = link_href.strip_prefix('/') {
        ("", stripped)
    } else {
        // dirname(source_relative) — everything before the last `/`.
        let dir = match source_relative.rfind('/') {
            Some(i) => &source_relative[..i],
            None => "",
        };
        (dir, link_href)
    };

    // Walk components and resolve `.` / `..`. We treat the input as
    // forward-slash; this avoids OS-specific `Path::components`
    // surprises (e.g. Windows backslash, drive prefixes) that don't
    // apply to URL paths.
    let mut stack: Vec<&str> = Vec::new();
    if !base_dir.is_empty() {
        for seg in base_dir.split('/') {
            if !seg.is_empty() && seg != "." {
                stack.push(seg);
            }
        }
    }
    for seg in rest.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                stack.pop();
            }
            _ => stack.push(seg),
        }
    }
    stack.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document_profile::DocumentProfile;
    use std::path::PathBuf;

    fn profile(source: &str, output_href: &str) -> DocumentProfile {
        DocumentProfile {
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some("T".to_string()),
            ..DocumentProfile::default()
        }
    }

    #[test]
    fn external_urls_pass_through_unchanged() {
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("https://example.com", None, None, None, &mut diags),
            "https://example.com"
        );
        assert_eq!(
            resolve_href_for_html("mailto:a@b.c", None, None, None, &mut diags),
            "mailto:a@b.c"
        );
        assert_eq!(
            resolve_href_for_html("//cdn.example.com/x", None, None, None, &mut diags),
            "//cdn.example.com/x"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn fragment_anchors_pass_through_unchanged() {
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("#section", None, None, None, &mut diags),
            "#section"
        );
        assert!(diags.is_empty());
    }

    /// Today's defensive no-resolver hit: returns bare `output_href`.
    /// (Production callers always pass a resolver — see the new
    /// `nav_href_relativizes_via_resolver_*` tests below.)
    #[test]
    fn qmd_href_rewrites_via_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", None, Some(&idx), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn qmd_miss_emits_diagnostic_with_source_label() {
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let out =
            resolve_href_for_html("missing.qmd", None, Some(&idx), Some("Navbar"), &mut diags);
        // Href preserved so the dangling link renders visibly.
        assert_eq!(out, "missing.qmd");
        assert_eq!(diags.len(), 1);
        // The source_label appears verbatim, followed by the expected
        // diagnostic shape.
        assert!(diags[0].title.starts_with("Navbar"));
        assert!(diags[0].title.contains("missing.qmd"));
    }

    #[test]
    fn miss_without_source_label_uses_generic_tag() {
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let _ = resolve_href_for_html("missing.qmd", None, Some(&idx), None, &mut diags);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.starts_with("Navigation"));
    }

    #[test]
    fn query_and_fragment_preserved_across_rewrite() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd#bio", None, Some(&idx), None, &mut diags),
            "about.html#bio"
        );
        assert_eq!(
            resolve_href_for_html("about.qmd?x=1", None, Some(&idx), None, &mut diags),
            "about.html?x=1"
        );
    }

    #[test]
    fn no_index_passes_raw_href_through() {
        let mut diags = Vec::new();
        // A .qmd-shaped href without an index is NOT a miss — there's
        // simply no lookup possible. Pass through; no diagnostic.
        assert_eq!(
            resolve_href_for_html("about.qmd", None, None, None, &mut diags),
            "about.qmd"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn non_qmd_miss_does_not_emit_diagnostic() {
        // An href like `assets/logo.png` shouldn't warn — it may
        // resolve as an ordinary static resource.
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let out = resolve_href_for_html("assets/logo.png", None, Some(&idx), None, &mut diags);
        assert_eq!(out, "assets/logo.png");
        assert!(diags.is_empty());
    }

    #[test]
    fn is_external_classification() {
        assert!(is_external("http://x"));
        assert!(is_external("https://x"));
        assert!(is_external("mailto:a@b.c"));
        assert!(is_external("tel:1234"));
        assert!(is_external("ftp://x"));
        assert!(is_external("//x"));
        assert!(!is_external("about.qmd"));
        assert!(!is_external("docs/api.qmd"));
        assert!(!is_external("#fragment"));
    }

    // ---- Phase 6: `resolve_to_project_root` (path normalization) ----
    //
    // Tests 8-15 from the Phase 6 sub-plan. Body-content link hrefs
    // are written relative to the source document's directory; this
    // helper joins them against `dirname(source_relative)` and
    // normalizes `..` / `.` components.

    /// Plan test 8: leading `/` strips to project-relative.
    #[test]
    fn path_normalize_leading_slash_strips() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "/about.qmd"),
            "about.qmd"
        );
        assert_eq!(
            resolve_to_project_root("index.qmd", "/foo/bar.qmd"),
            "foo/bar.qmd"
        );
    }

    /// Plan test 9: doc-relative path with no `..` joins with parent.
    #[test]
    fn path_normalize_doc_relative_no_dotdot() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "foo.qmd"),
            "docs/foo.qmd"
        );
    }

    /// Plan test 10: `..` walks up one parent.
    #[test]
    fn path_normalize_dotdot_to_parent() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "../about.qmd"),
            "about.qmd"
        );
    }

    /// Plan test 11: multiple `..` walks up multiple parents.
    #[test]
    fn path_normalize_multiple_dotdot() {
        assert_eq!(
            resolve_to_project_root("a/b/c.qmd", "../../about.qmd"),
            "about.qmd"
        );
    }

    /// Plan test 12: `.` is a no-op.
    #[test]
    fn path_normalize_dot_no_op() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "./foo.qmd"),
            "docs/foo.qmd"
        );
    }

    /// Plan test 13: walking above the project root clamps at root.
    #[test]
    fn path_normalize_clamp_above_root() {
        assert_eq!(
            resolve_to_project_root("a/b.qmd", "../../../foo.qmd"),
            "foo.qmd"
        );
    }

    /// Plan test 14: subdir join.
    #[test]
    fn path_normalize_subdir() {
        assert_eq!(
            resolve_to_project_root("docs/api.qmd", "sub/foo.qmd"),
            "docs/sub/foo.qmd"
        );
    }

    /// Plan test 15: source at project root joins cleanly.
    #[test]
    fn path_normalize_root_source() {
        assert_eq!(
            resolve_to_project_root("index.qmd", "about.qmd"),
            "about.qmd"
        );
    }

    // ---- Phase 6: `resolve_doc_relative_href` ----
    //
    // Tests 16-28. Body-content link rewriting helper that builds
    // on `resolve_to_project_root` plus the project index lookup
    // and the resolver's page-relative URL math.

    use crate::resource_resolver::ResourceResolverContext;

    fn website_resolver(page: &str) -> ResourceResolverContext {
        // `page` is the page's project-relative output href, e.g.
        // "docs/api.html". Construct a website-flavored resolver
        // pinned at `/project/_site/{page}`.
        let page_output = format!("/project/_site/{}", page);
        let stem = std::path::Path::new(page)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("index");
        ResourceResolverContext::website("/project/_site", page_output, "site_libs", stem)
    }

    /// Plan test 16: external URL passes through unchanged.
    #[test]
    fn body_href_external_passes_through() {
        let mut diags = Vec::new();
        let r = website_resolver("index.html");
        assert_eq!(
            resolve_doc_relative_href(
                "https://example.com",
                "index.qmd",
                Some(&r),
                None,
                None,
                &mut diags
            ),
            "https://example.com"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 17: fragment-only anchor passes through.
    #[test]
    fn body_href_fragment_only_passes_through() {
        let mut diags = Vec::new();
        let r = website_resolver("index.html");
        assert_eq!(
            resolve_doc_relative_href("#section", "index.qmd", Some(&r), None, None, &mut diags),
            "#section"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 18: simple `.qmd` body link from the root.
    #[test]
    fn body_href_qmd_hits_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 19: doc-relative `..` from a nested source.
    #[test]
    fn body_href_doc_relative_qmd_hits_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "../about.qmd",
                "docs/api.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../about.html"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 20: leading `/` (absolute project-root) from nested.
    #[test]
    fn body_href_absolute_qmd_hits_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "/about.qmd",
                "docs/api.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../about.html"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 21: subdir target from project root.
    #[test]
    fn body_href_subdir_qmd() {
        let idx = ProjectIndex::new(vec![profile("docs/api.qmd", "docs/api.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "docs/api.qmd",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "docs/api.html"
        );
    }

    /// Plan test 22: hash fragment preserved.
    #[test]
    fn body_href_preserves_fragment() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd#bio",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html#bio"
        );
    }

    /// Plan test 23: query string preserved.
    #[test]
    fn body_href_preserves_query() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd?x=1",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html?x=1"
        );
    }

    /// Plan test 24: query + fragment together.
    #[test]
    fn body_href_preserves_query_and_fragment() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href(
                "about.qmd?x=1#bio",
                "index.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "about.html?x=1#bio"
        );
    }

    /// Plan test 25: missing `.qmd` emits diagnostic; href preserved.
    #[test]
    fn body_href_qmd_miss_emits_diagnostic() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        let out = resolve_doc_relative_href(
            "missing.qmd",
            "index.qmd",
            Some(&r),
            Some(&idx),
            Some("Body link"),
            &mut diags,
        );
        assert_eq!(out, "missing.qmd");
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.starts_with("Body link"));
        assert!(diags[0].title.contains("missing.qmd"));
    }

    /// Plan test 26: non-`.qmd` miss no diagnostic.
    #[test]
    fn body_href_non_qmd_miss_no_diagnostic() {
        let idx = ProjectIndex::new(vec![]);
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        let out = resolve_doc_relative_href(
            "assets/logo.png",
            "index.qmd",
            Some(&r),
            Some(&idx),
            Some("Body link"),
            &mut diags,
        );
        assert_eq!(out, "assets/logo.png");
        assert!(diags.is_empty());
    }

    /// Plan test 27: no project index = standalone render = no rewrite.
    #[test]
    fn body_href_no_index_passes_through() {
        let r = website_resolver("index.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href("about.qmd", "index.qmd", Some(&r), None, None, &mut diags),
            "about.qmd"
        );
        assert!(diags.is_empty());
    }

    /// Plan test 28: no resolver falls back to bare `output_href`.
    /// Verifies the helper degrades gracefully when the resolver
    /// hasn't been wired in (defensive — not exercised in the
    /// production pipeline, but unit tests / out-of-band callers
    /// may construct a `RenderContext` without one).
    #[test]
    fn body_href_no_resolver_falls_back_to_output_href() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_doc_relative_href("about.qmd", "index.qmd", None, Some(&idx), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    // ---- Phase 8: `resolve_doc_relative_target` ----
    //
    // Pass-1 / static counterpart to `resolve_doc_relative_href`.
    // Returns the project-relative target path for any internal
    // `.qmd` reference (no index check — the dependency-graph
    // builder applies the existence filter). See
    // `claude-notes/designs/body-link-resolution-contract.md`.

    #[test]
    fn target_external_returns_none() {
        assert_eq!(
            resolve_doc_relative_target("https://example.com", "index.qmd"),
            None
        );
        assert_eq!(
            resolve_doc_relative_target("mailto:a@b.c", "index.qmd"),
            None
        );
        assert_eq!(
            resolve_doc_relative_target("//cdn.example.com/x", "index.qmd"),
            None
        );
    }

    #[test]
    fn target_fragment_only_returns_none() {
        assert_eq!(resolve_doc_relative_target("#section", "index.qmd"), None);
    }

    #[test]
    fn target_non_qmd_returns_none() {
        assert_eq!(
            resolve_doc_relative_target("assets/logo.png", "index.qmd"),
            None
        );
    }

    #[test]
    fn target_simple_qmd_resolves() {
        assert_eq!(
            resolve_doc_relative_target("about.qmd", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_doc_relative_dotdot_resolves() {
        assert_eq!(
            resolve_doc_relative_target("../about.qmd", "docs/api.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_leading_slash_strips_to_project_root() {
        assert_eq!(
            resolve_doc_relative_target("/about.qmd", "docs/api.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_strips_query_and_fragment_before_lookup() {
        assert_eq!(
            resolve_doc_relative_target("about.qmd#bio", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
        assert_eq!(
            resolve_doc_relative_target("about.qmd?x=1", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
        assert_eq!(
            resolve_doc_relative_target("about.qmd?x=1#bio", "index.qmd"),
            Some(PathBuf::from("about.qmd"))
        );
    }

    #[test]
    fn target_unresolvable_qmd_returns_path() {
        // After the Phase-8.2 simplification, Pass-1 returns the
        // resolved project-relative path regardless of whether
        // the target is in the index. The dependency-graph
        // builder filters; Pass-1 doesn't.
        assert_eq!(
            resolve_doc_relative_target("missing.qmd", "index.qmd"),
            Some(PathBuf::from("missing.qmd"))
        );
    }

    /// Pass-1 / Pass-2 still agree on the *project-relative path*
    /// when both report a hit. Pass-2 additionally requires the
    /// path to be in the index to rewrite; Pass-1 does not. The
    /// dependency-graph builder applies the index filter so the
    /// edges it emits reflect Pass-2's rewrite set exactly.
    #[test]
    fn pass1_pass2_agree_on_resolved_path_when_both_hit() {
        let idx = ProjectIndex::new(vec![
            profile("about.qmd", "about.html"),
            profile("docs/api.qmd", "docs/api.html"),
        ]);
        let r = website_resolver("index.html");

        let cases: &[(&str, &str)] = &[
            ("index.qmd", "about.qmd"),
            ("index.qmd", "docs/api.qmd"),
            ("docs/api.qmd", "../about.qmd"),
            ("docs/api.qmd", "/about.qmd"),
            ("index.qmd", "about.qmd#bio"),
            ("index.qmd", "about.qmd?x=1"),
        ];

        for (source, raw) in cases {
            let p1 = resolve_doc_relative_target(raw, source);
            let mut diags = Vec::new();
            let p2 = resolve_doc_relative_href(raw, source, Some(&r), Some(&idx), None, &mut diags);

            let target = p1.expect("Pass-1 should resolve internal .qmd refs");
            // Pass-2 should have rewritten the link.
            assert_ne!(
                p2.as_str(),
                *raw,
                "case ({}, {}): Pass-2 should rewrite when target is in index",
                source,
                raw
            );
            // The resolved path Pass-1 returns matches a profile in the index.
            let prof = idx.lookup_by_source(&target).unwrap();
            let core = prof.output_href.as_str();
            let p2_path: String = p2
                .find(|c| c == '#' || c == '?')
                .map(|i| p2[..i].to_string())
                .unwrap_or_else(|| p2.clone());
            assert!(
                p2_path.ends_with(core),
                "case ({}, {}): Pass-2 output {:?} should end with target output_href {:?}",
                source,
                raw,
                p2_path,
                core
            );
        }
    }

    // ---- bd-swpy: navigation hrefs relativized via resolver ----
    //
    // Mirrors the body-link path: when a `ProjectIndex` lookup
    // succeeds and a `ResourceResolverContext` is attached, the
    // returned URL must be page-relative (the same shape
    // `resolve_doc_relative_href` produces). Without this the nav
    // helper emits project-root-relative output that 404s from
    // pages in subdirectories. See
    // `claude-notes/plans/2026-04-29-bd-swpy-nav-href-relativization.md`.

    /// bd-swpy test 1 — depth-1 page links to a root-level target
    /// via the resolver. Output must walk up one level.
    #[test]
    fn nav_href_relativizes_via_resolver_at_depth_one() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", Some(&r), Some(&idx), None, &mut diags),
            "../about.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 2 — depth-2 page links to a target one
    /// directory deep. Output walks up two levels then descends.
    #[test]
    fn nav_href_relativizes_via_resolver_at_depth_two() {
        let idx = ProjectIndex::new(vec![profile(
            "guide/installation.qmd",
            "guide/installation.html",
        )]);
        let r = website_resolver("docs/internals/architecture.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html(
                "guide/installation.qmd",
                Some(&r),
                Some(&idx),
                None,
                &mut diags
            ),
            "../../guide/installation.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 3 — depth-1 page in one subtree links to a
    /// target in a sibling subtree (the multi-sidebar swap case
    /// from `examples/websites/03-nested-sidebar`).
    #[test]
    fn nav_href_relativizes_subdir_to_subdir() {
        let idx = ProjectIndex::new(vec![profile("reference/api.qmd", "reference/api.html")]);
        let r = website_resolver("guide/installation.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("reference/api.qmd", Some(&r), Some(&idx), None, &mut diags),
            "../reference/api.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 4 — defensive no-resolver fallback. With no
    /// resolver attached, hits return the bare project-root-relative
    /// `output_href` (today's behaviour). Production callers always
    /// pass a resolver; this branch covers unit tests / out-of-band
    /// callers.
    #[test]
    fn nav_href_no_resolver_falls_back_to_bare_output_href() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", None, Some(&idx), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    /// bd-swpy test 5 — query / fragment tail is preserved across
    /// the resolver-relativized rewrite. Tail is appended *after*
    /// the resolver call, identical to the body-link helper.
    #[test]
    fn nav_href_preserves_query_and_fragment_through_resolver() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let r = website_resolver("docs/api.html");
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd#bio", Some(&r), Some(&idx), None, &mut diags),
            "../about.html#bio"
        );
        assert_eq!(
            resolve_href_for_html("about.qmd?x=1", Some(&r), Some(&idx), None, &mut diags),
            "../about.html?x=1"
        );
        assert_eq!(
            resolve_href_for_html("about.qmd?x=1#bio", Some(&r), Some(&idx), None, &mut diags),
            "../about.html?x=1#bio"
        );
        assert!(diags.is_empty());
    }
}
