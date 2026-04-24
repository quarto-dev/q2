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
}
