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

use std::path::Path;

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
///   Query strings and fragments are preserved across the rewrite.
/// - Source-path-shaped misses (looks like `*.qmd` but no matching
///   profile) emit a warning diagnostic naming the `source_label` and
///   the missing target. The raw href is preserved in the output so
///   the dangling link is at least visible to the reader.
///
/// `source_label` is a human-readable tag (`"Sidebar 'docs'"`,
/// `"Navbar"`, `"Page footer"`) — it appears verbatim in the
/// diagnostic so the user can locate the offending config.
pub fn resolve_href_for_html(
    raw: &str,
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
            return format!("{}{}", profile.output_href, tail);
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

/// Resolve a body-content href to a relative URL.
///
/// Companion to [`resolve_href_for_html`] for **body** content
/// (the inline `Link` nodes parsed from markdown). The two helpers
/// differ in their input/output normalization:
///
/// | Helper | Input | Output |
/// |--------|-------|--------|
/// | `resolve_href_for_html` | project-root-relative | project-root-relative |
/// | `resolve_doc_relative_href` | source-doc-relative | page-relative |
///
/// Phase 6 of the website-projects epic. See
/// `claude-notes/plans/2026-04-24-websites-phase-6.md` Decisions 3,
/// 4, 9, 10.
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
    use crate::document_profile::{DOCUMENT_PROFILE_VERSION, DocumentProfile};
    use pampa::toc::TocEntry;
    use std::path::PathBuf;

    fn profile(source: &str, output_href: &str) -> DocumentProfile {
        DocumentProfile {
            profile_version: DOCUMENT_PROFILE_VERSION,
            source_path: PathBuf::from(source),
            output_href: output_href.to_string(),
            format_id: "html".to_string(),
            title: Some("T".to_string()),
            subtitle: None,
            description: None,
            authors: Vec::new(),
            date: None,
            categories: Vec::new(),
            keywords: Vec::new(),
            image: None,
            draft: false,
            order: None,
            outline: Vec::<TocEntry>::new(),
        }
    }

    #[test]
    fn external_urls_pass_through_unchanged() {
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("https://example.com", None, None, &mut diags),
            "https://example.com"
        );
        assert_eq!(
            resolve_href_for_html("mailto:a@b.c", None, None, &mut diags),
            "mailto:a@b.c"
        );
        assert_eq!(
            resolve_href_for_html("//cdn.example.com/x", None, None, &mut diags),
            "//cdn.example.com/x"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn fragment_anchors_pass_through_unchanged() {
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("#section", None, None, &mut diags),
            "#section"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn qmd_href_rewrites_via_index() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd", Some(&idx), None, &mut diags),
            "about.html"
        );
        assert!(diags.is_empty());
    }

    #[test]
    fn qmd_miss_emits_diagnostic_with_source_label() {
        let idx = ProjectIndex::new(vec![]);
        let mut diags = Vec::new();
        let out = resolve_href_for_html("missing.qmd", Some(&idx), Some("Navbar"), &mut diags);
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
        let _ = resolve_href_for_html("missing.qmd", Some(&idx), None, &mut diags);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].title.starts_with("Navigation"));
    }

    #[test]
    fn query_and_fragment_preserved_across_rewrite() {
        let idx = ProjectIndex::new(vec![profile("about.qmd", "about.html")]);
        let mut diags = Vec::new();
        assert_eq!(
            resolve_href_for_html("about.qmd#bio", Some(&idx), None, &mut diags),
            "about.html#bio"
        );
        assert_eq!(
            resolve_href_for_html("about.qmd?x=1", Some(&idx), None, &mut diags),
            "about.html?x=1"
        );
    }

    #[test]
    fn no_index_passes_raw_href_through() {
        let mut diags = Vec::new();
        // A .qmd-shaped href without an index is NOT a miss — there's
        // simply no lookup possible. Pass through; no diagnostic.
        assert_eq!(
            resolve_href_for_html("about.qmd", None, None, &mut diags),
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
        let out = resolve_href_for_html("assets/logo.png", Some(&idx), None, &mut diags);
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
}
