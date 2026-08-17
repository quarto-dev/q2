/*
 * project/listing/glob_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Listing adapter for the shared glob API (GH #456, bd-v7ixzsp5;
//! generalized in bd-mt7a6uc4).
//!
//! The semantics — provenance-resolved base directories, lexical
//! `../` normalization with project-root clamping, `!` negation,
//! leading-`/` re-anchoring, single-view matching — live in
//! [`crate::glob`] and are shared with `project.render`,
//! `resources:`, and `sidebar.auto:`. This module only translates
//! listing's [`ListingContents`] into that API and pins the
//! listing-specific option set ([`GlobOptions::LISTING`]: bare
//! directories match beneath, and a negation-only `contents:`
//! defaults its positive set to the host directory's `*.qmd`
//! siblings).
//!
//! Both consumption points — [`ListingGenerateTransform`](crate::transforms::ListingGenerateTransform)
//! at render time and [`ProjectDependencyGraph::build`](crate::project::dependency_graph::ProjectDependencyGraph)
//! for the edge set — resolve through here, so the two cannot drift.

use std::path::Path;

use quarto_source_map::SourceContext;

use super::ListingContents;
use crate::glob::{BaseDirContext, GlobOptions, GlobResolution, RawGlob, resolve_patterns};

/// Resolve a listing's `contents:` entries to project-relative
/// patterns.
///
/// `host_dir` is the host document's project-relative directory
/// (forward slashes, `""` for the project root) — the fallback base
/// for values whose declaring file cannot be recovered, and the base
/// for the negation-only default. `project_dir` is the absolute
/// project root. Inline-record entries contribute nothing (the
/// parser already emitted `Q-12-2`).
///
/// Patterns that escape the project root land in
/// [`GlobResolution::escaped`] (reported as `Q-12-17`); patterns the
/// glob engine rejects land in [`GlobResolution::invalid`]. Neither
/// matches anything.
pub fn resolve_content_globs(
    contents: &[ListingContents],
    source_context: Option<&SourceContext>,
    project_dir: &Path,
    host_dir: &str,
) -> GlobResolution {
    let raws = contents.iter().filter_map(|entry| match entry {
        ListingContents::Glob { pattern, source } => {
            Some(RawGlob::new(pattern.clone(), source.clone()))
        }
        _ => None,
    });

    let ctx = BaseDirContext {
        source_context,
        project_dir,
        fallback_dir: host_dir,
    };

    resolve_patterns(raws, &ctx, &GlobOptions::LISTING)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glob::GlobPattern;
    use quarto_source_map::{By, FileId, SourceInfo};

    fn glob(pattern: &str, source: SourceInfo) -> ListingContents {
        ListingContents::Glob {
            pattern: pattern.to_string(),
            source,
        }
    }

    fn programmatic() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    /// A SourceContext with the host doc as FileId(0) plus a
    /// hash-registered YAML layer, mirroring what the parse +
    /// MetadataMergeStage leave behind.
    fn context_with_layer(doc: &str, layer: &str) -> (SourceContext, FileId) {
        let mut sc = SourceContext::new();
        let doc_id = sc.add_file(doc.to_string(), None);
        assert_eq!(doc_id, FileId(0));
        let layer_id = quarto_yaml::file_id_for_filename(layer);
        sc.add_file_with_id(layer_id, layer.to_string(), None);
        (sc, layer_id)
    }

    #[test]
    fn frontmatter_glob_resolves_against_doc_dir() {
        let (sc, _) = context_with_layer("/proj/sub/index.qmd", "/proj/_quarto.yml");
        let source = SourceInfo::substring(SourceInfo::original(FileId(0), 0, 100), 10, 20);
        let r = resolve_content_globs(
            &[glob("*.qmd", source)],
            Some(&sc),
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("sub/*.qmd")]);
        assert!(r.escaped.is_empty() && r.invalid.is_empty());
    }

    #[test]
    fn metadata_layer_glob_resolves_against_layer_dir() {
        let (sc, layer_id) =
            context_with_layer("/proj/blog/deep/index.qmd", "/proj/blog/_metadata.yml");
        let source = SourceInfo::original(layer_id, 0, 10);
        let r = resolve_content_globs(
            &[glob("deep/*.qmd", source)],
            Some(&sc),
            Path::new("/proj"),
            "blog/deep",
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("blog/deep/*.qmd")]);
    }

    #[test]
    fn project_config_glob_resolves_against_root() {
        let (sc, layer_id) = context_with_layer("/proj/sub/viewer.qmd", "/proj/_quarto.yml");
        let source = SourceInfo::original(layer_id, 0, 10);
        let r = resolve_content_globs(
            &[glob("posts/*.qmd", source)],
            Some(&sc),
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("posts/*.qmd")]);
    }

    #[test]
    fn unresolvable_provenance_falls_back_to_host_dir() {
        let r = resolve_content_globs(
            &[glob("*.qmd", programmatic())],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("sub/*.qmd")]);
    }

    #[test]
    fn escaping_glob_is_reported_not_matched() {
        let r = resolve_content_globs(
            &[glob("../../*.qmd", programmatic())],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert!(r.globs.is_empty());
        assert_eq!(r.escaped.len(), 1);
        assert_eq!(r.escaped[0].raw, "../../*.qmd");
    }

    #[test]
    fn negation_strips_bang_and_sets_flag() {
        let r = resolve_content_globs(
            &[
                glob("*.qmd", programmatic()),
                glob("!p2.qmd", programmatic()),
            ],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(
            r.globs,
            vec![
                GlobPattern::positive("sub/*.qmd"),
                GlobPattern::negated("sub/p2.qmd"),
            ]
        );
    }

    #[test]
    fn negation_only_injects_host_default_positive() {
        let r = resolve_content_globs(
            &[glob("!p2.qmd", programmatic())],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(
            r.globs,
            vec![
                GlobPattern::positive("sub/*.qmd"),
                GlobPattern::negated("sub/p2.qmd"),
            ]
        );
    }

    /// Inline-record entries are not globs; the parser already
    /// diagnosed them (`Q-12-2`).
    #[test]
    fn inline_records_contribute_nothing() {
        let r = resolve_content_globs(
            &[ListingContents::Inline(Default::default())],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert!(r.is_empty());
    }

    /// A subdirectory host can still reach project-root files by
    /// anchoring the pattern explicitly (D2) — the escape hatch for
    /// the #456 behavior change.
    #[test]
    fn leading_slash_reaches_the_project_root_from_a_subdir_host() {
        let r = resolve_content_globs(
            &[glob("/posts/*.qmd", programmatic())],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(r.globs, vec![GlobPattern::positive("posts/*.qmd")]);
    }

    /// Character classes reach the matcher intact (bd-mt7a6uc4 D1);
    /// before the shared API they silently matched nothing here.
    #[test]
    fn character_class_patterns_resolve_and_match() {
        let r = resolve_content_globs(
            &[glob("p[0-9].qmd", programmatic())],
            None,
            Path::new("/proj"),
            "posts",
        );
        let set = r.compile(&GlobOptions::LISTING).expect("compiles");
        assert!(set.matches("posts/p1.qmd"));
        assert!(!set.matches("posts/px.qmd"));
    }
}
