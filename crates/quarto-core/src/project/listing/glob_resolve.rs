/*
 * project/listing/glob_resolve.rs
 * Copyright (c) 2026 Posit, PBC
 */

//! Base-directory resolution and matching for listing `contents:`
//! globs (GH #456, bd-v7ixzsp5).
//!
//! A `contents:` glob resolves relative to the directory of the file
//! where it was **written**:
//!
//! - document front matter → the host document's directory;
//! - a directory `_metadata.yml` → that file's directory;
//! - the project `_quarto.yml` → the project root.
//!
//! The declaring file is recovered from the glob's `ConfigValue`
//! provenance: [`SourceInfo::root_file_id`] gives the `FileId` the
//! value was parsed from, and the document's
//! [`SourceContext`](quarto_source_map::SourceContext) maps it back
//! to a path ([`MetadataMergeStage`](crate::stage::stages::MetadataMergeStage)
//! registers the YAML metadata layers there for exactly this
//! lookup). Values with no recoverable file — runtime `--metadata`,
//! programmatic config, extension metadata — fall back to the host
//! document's directory.
//!
//! Matching is **single-view**: the pattern is joined to its base
//! directory, lexically normalized, and matched against each
//! candidate's project-relative path. There is no project-relative
//! fallback (the pre-#456 dual-view rule let `*.qmd` in `sub/`
//! match project-root files). A pattern that normalizes outside the
//! project root matches nothing and is reported to the caller so
//! the render transform can emit `Q-12-17`.
//!
//! Negation: a `!`-prefixed pattern excludes matches. An item is
//! included iff it matches at least one positive pattern and no
//! negative pattern. When `contents:` holds only negative patterns,
//! the positive set defaults to `*.qmd` in the host directory.
//!
//! This module is intended as the seed of a shared, base-directory-
//! anchored glob-expansion API for q2 (plan decision 3); other glob
//! consumers (`project.render`, resources) can migrate onto it once
//! the shape settles.

use std::path::Path;

use quarto_source_map::{SourceContext, SourceInfo};
use serde::{Deserialize, Serialize};

use super::ListingContents;
use crate::project::discovery::glob_match_path_or_dir;

/// One resolved `contents:` glob: a normalized project-relative
/// pattern plus its negation flag. This is the shape stored on
/// [`DocumentProfile::listing_content_globs`](crate::document_profile::DocumentProfile)
/// (resolution happens once, at profile-extraction / listing-parse
/// time; consumers only match).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListingContentGlob {
    /// Normalized project-relative pattern (forward slashes, no
    /// `.`/`..` segments), e.g. `"sub/*.qmd"`.
    pub pattern: String,
    /// True for `!`-prefixed patterns: matches are excluded.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub negated: bool,
}

/// A `contents:` glob whose normalized form escapes the project
/// root (e.g. `../../*.qmd` written next to the root). It matches
/// nothing; the render transform reports it as `Q-12-17`.
#[derive(Debug, Clone, PartialEq)]
pub struct EscapedContentGlob {
    /// The pattern as the user wrote it (including any `!`).
    pub raw: String,
    /// Provenance of the offending YAML scalar, for the diagnostic
    /// span.
    pub source: SourceInfo,
}

/// Result of resolving a listing's `contents:` entries.
#[derive(Debug, Clone, Default)]
pub struct GlobResolution {
    /// Usable patterns (positive and negative), project-relative.
    pub globs: Vec<ListingContentGlob>,
    /// Patterns that escaped the project root (diagnose, match
    /// nothing).
    pub escaped: Vec<EscapedContentGlob>,
}

/// Resolve each glob entry of a listing's `contents:` to a
/// project-relative pattern.
///
/// `host_dir` is the host document's project-relative directory
/// (forward slashes, `""` for the project root) — the fallback base
/// and the base for the negation-only default. `project_dir` is the
/// absolute project root, used to relativize absolute paths found
/// in the `SourceContext`. Inline-record entries contribute
/// nothing (the parser already emitted `Q-12-2`).
pub fn resolve_content_globs(
    contents: &[ListingContents],
    source_context: Option<&SourceContext>,
    project_dir: &Path,
    host_dir: &str,
) -> GlobResolution {
    let mut out = GlobResolution::default();

    for entry in contents {
        let ListingContents::Glob {
            pattern: raw,
            source,
        } = entry
        else {
            continue;
        };
        let (negated, pattern) = match raw.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, raw.as_str()),
        };
        let base_dir = base_dir_for(source, source_context, project_dir, host_dir);
        match join_and_normalize(&base_dir, pattern) {
            Some(resolved) => out.globs.push(ListingContentGlob {
                pattern: resolved,
                negated,
            }),
            None => out.escaped.push(EscapedContentGlob {
                raw: raw.clone(),
                source: source.clone(),
            }),
        }
    }

    // Negation-only contents: the positive set defaults to the
    // host-directory siblings (`*.qmd`), mirroring the
    // absent-`contents:` default.
    if !out.globs.is_empty() && out.globs.iter().all(|g| g.negated) {
        // `join_and_normalize` cannot fail here: `host_dir` is
        // already a normalized project-relative directory.
        if let Some(default_pattern) = join_and_normalize(host_dir, "*.qmd") {
            out.globs.insert(
                0,
                ListingContentGlob {
                    pattern: default_pattern,
                    negated: false,
                },
            );
        }
    }

    out
}

/// True iff `candidate` (a project-relative forward-slash path)
/// matches at least one positive pattern and no negative pattern.
/// Bare-directory patterns match everything beneath them (the
/// [`glob_match_path_or_dir`] rule).
pub fn item_matches(globs: &[ListingContentGlob], candidate: &str) -> bool {
    globs
        .iter()
        .any(|g| !g.negated && glob_match_path_or_dir(&g.pattern, candidate))
        && !globs
            .iter()
            .any(|g| g.negated && glob_match_path_or_dir(&g.pattern, candidate))
}

/// Project-relative directory (forward slashes, `""` for the root)
/// of the file a value was written in, per its `SourceInfo`
/// provenance. Falls back to `host_dir` when the file cannot be
/// recovered (generated/programmatic values, unregistered files,
/// paths outside the project).
fn base_dir_for(
    source: &SourceInfo,
    source_context: Option<&SourceContext>,
    project_dir: &Path,
    host_dir: &str,
) -> String {
    let resolved = source
        .root_file_id()
        .and_then(|id| source_context?.get_file(id))
        .and_then(|f| project_relative_dir_of(&f.path, project_dir));
    resolved.unwrap_or_else(|| host_dir.to_string())
}

/// Directory of `file_path` as a project-relative forward-slash
/// string. Absolute paths are relativized against `project_dir`;
/// already-relative paths are taken as project-relative. Returns
/// `None` for placeholder names (`<unknown>`, empty) and for paths
/// outside the project.
fn project_relative_dir_of(file_path: &str, project_dir: &Path) -> Option<String> {
    if file_path.is_empty() || file_path.starts_with('<') {
        return None;
    }
    let path = Path::new(file_path);
    let relative = if quarto_util::is_rooted(path) {
        path.strip_prefix(project_dir).ok()?
    } else {
        path
    };
    let dir = relative.parent().unwrap_or(Path::new(""));
    let mut segments: Vec<&str> = Vec::new();
    for comp in dir.components() {
        match comp {
            std::path::Component::Normal(os) => segments.push(os.to_str()?),
            std::path::Component::CurDir => {}
            // `..` or a root inside a supposedly project-relative
            // path — not resolvable to a project-relative dir.
            _ => return None,
        }
    }
    Some(segments.join("/"))
}

/// Join `pattern` onto `base_dir` and lexically normalize the
/// result: `.` segments drop, `..` segments pop. Returns `None`
/// when a `..` would climb above the project root. Both inputs use
/// forward slashes (backslashes are normalized for safety, matching
/// the matcher's tolerance).
fn join_and_normalize(base_dir: &str, pattern: &str) -> Option<String> {
    let pattern = pattern.replace('\\', "/");
    let mut segments: Vec<&str> = Vec::new();
    let joined = if base_dir.is_empty() {
        pattern.clone()
    } else {
        format!("{base_dir}/{pattern}")
    };
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    Some(segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_source_map::{By, FileId};

    fn glob(pattern: &str, source: SourceInfo) -> ListingContents {
        ListingContents::Glob {
            pattern: pattern.to_string(),
            source,
        }
    }

    fn programmatic() -> SourceInfo {
        SourceInfo::generated(By::programmatic_config())
    }

    // ── join_and_normalize ──────────────────────────────────────

    #[test]
    fn join_empty_base_is_identity() {
        assert_eq!(join_and_normalize("", "*.qmd"), Some("*.qmd".into()));
        assert_eq!(
            join_and_normalize("", "posts/**/*.qmd"),
            Some("posts/**/*.qmd".into())
        );
    }

    #[test]
    fn join_prefixes_base_dir() {
        assert_eq!(join_and_normalize("sub", "*.qmd"), Some("sub/*.qmd".into()));
        assert_eq!(
            join_and_normalize("blog", "deep/*.qmd"),
            Some("blog/deep/*.qmd".into())
        );
    }

    #[test]
    fn join_normalizes_dot_and_dotdot() {
        assert_eq!(
            join_and_normalize("sub", "../rootpost.qmd"),
            Some("rootpost.qmd".into())
        );
        assert_eq!(
            join_and_normalize("sub", "./*.qmd"),
            Some("sub/*.qmd".into())
        );
        assert_eq!(
            join_and_normalize("a/b", "../c/*.qmd"),
            Some("a/c/*.qmd".into())
        );
        // `..` inside the pattern body normalizes too.
        assert_eq!(
            join_and_normalize("", "posts/../notes/*.qmd"),
            Some("notes/*.qmd".into())
        );
    }

    #[test]
    fn join_escaping_project_root_is_none() {
        assert_eq!(join_and_normalize("", "../*.qmd"), None);
        assert_eq!(join_and_normalize("sub", "../../*.qmd"), None);
        assert_eq!(join_and_normalize("a/b", "../../../x.qmd"), None);
    }

    // ── project_relative_dir_of ─────────────────────────────────

    #[test]
    fn dir_of_absolute_path_inside_project() {
        let project = Path::new("/proj");
        assert_eq!(
            project_relative_dir_of("/proj/blog/_metadata.yml", project),
            Some("blog".into())
        );
        assert_eq!(
            project_relative_dir_of("/proj/_quarto.yml", project),
            Some(String::new())
        );
    }

    #[test]
    fn dir_of_relative_path_is_project_relative() {
        let project = Path::new("/proj");
        assert_eq!(
            project_relative_dir_of("sub/index.qmd", project),
            Some("sub".into())
        );
    }

    #[test]
    fn dir_of_placeholder_or_outside_is_none() {
        let project = Path::new("/proj");
        assert_eq!(project_relative_dir_of("<unknown>", project), None);
        assert_eq!(project_relative_dir_of("", project), None);
        assert_eq!(project_relative_dir_of("/elsewhere/doc.qmd", project), None);
    }

    // ── resolve_content_globs ───────────────────────────────────

    /// A SourceContext with the host doc as FileId(0) plus a
    /// hash-registered YAML layer, mirroring what the parse +
    /// MetadataMergeStage leave behind.
    fn context_with_layer(doc: &str, layer: &str) -> (SourceContext, FileId) {
        let mut sc = SourceContext::new();
        // Overwrite the FileId(0) slot convention: a fresh context
        // in production is created via ASTContext::with_filename,
        // which adds the doc as FileId(0). SourceContext::new()
        // pre-adds nothing here, so add the doc first.
        let doc_id = sc.add_file(doc.to_string(), None);
        assert_eq!(doc_id, FileId(0));
        let layer_id = quarto_yaml::file_id_for_filename(layer);
        sc.add_file_with_id(layer_id, layer.to_string(), None);
        (sc, layer_id)
    }

    #[test]
    fn frontmatter_glob_resolves_against_doc_dir() {
        let (sc, _) = context_with_layer("/proj/sub/index.qmd", "/proj/_quarto.yml");
        // Front matter values are Substrings into FileId(0).
        let source = SourceInfo::substring(SourceInfo::original(FileId(0), 0, 100), 10, 20);
        let r = resolve_content_globs(
            &[glob("*.qmd", source)],
            Some(&sc),
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(
            r.globs,
            vec![ListingContentGlob {
                pattern: "sub/*.qmd".into(),
                negated: false
            }]
        );
        assert!(r.escaped.is_empty());
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
        assert_eq!(
            r.globs,
            vec![ListingContentGlob {
                pattern: "blog/deep/*.qmd".into(),
                negated: false
            }]
        );
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
        assert_eq!(
            r.globs,
            vec![ListingContentGlob {
                pattern: "posts/*.qmd".into(),
                negated: false
            }]
        );
    }

    #[test]
    fn unresolvable_provenance_falls_back_to_host_dir() {
        let r = resolve_content_globs(
            &[glob("*.qmd", programmatic())],
            None,
            Path::new("/proj"),
            "sub",
        );
        assert_eq!(
            r.globs,
            vec![ListingContentGlob {
                pattern: "sub/*.qmd".into(),
                negated: false
            }]
        );
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
                ListingContentGlob {
                    pattern: "sub/*.qmd".into(),
                    negated: false
                },
                ListingContentGlob {
                    pattern: "sub/p2.qmd".into(),
                    negated: true
                },
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
                ListingContentGlob {
                    pattern: "sub/*.qmd".into(),
                    negated: false
                },
                ListingContentGlob {
                    pattern: "sub/p2.qmd".into(),
                    negated: true
                },
            ]
        );
    }

    // ── item_matches ────────────────────────────────────────────

    fn pos(p: &str) -> ListingContentGlob {
        ListingContentGlob {
            pattern: p.into(),
            negated: false,
        }
    }

    fn neg(p: &str) -> ListingContentGlob {
        ListingContentGlob {
            pattern: p.into(),
            negated: true,
        }
    }

    #[test]
    fn matches_positive_only() {
        let globs = vec![pos("sub/*.qmd")];
        assert!(item_matches(&globs, "sub/p1.qmd"));
        assert!(!item_matches(&globs, "about.qmd"));
        assert!(!item_matches(&globs, "sub/deep/p1.qmd"));
    }

    #[test]
    fn negation_excludes() {
        let globs = vec![pos("sub/*.qmd"), neg("sub/p2.qmd")];
        assert!(item_matches(&globs, "sub/p1.qmd"));
        assert!(!item_matches(&globs, "sub/p2.qmd"));
    }

    #[test]
    fn bare_directory_positive_matches_beneath() {
        let globs = vec![pos("posts")];
        assert!(item_matches(&globs, "posts/welcome/index.qmd"));
        assert!(!item_matches(&globs, "posts-archive/old.qmd"));
    }

    #[test]
    fn bare_directory_negation_excludes_beneath() {
        let globs = vec![pos("posts"), neg("posts/drafts")];
        assert!(item_matches(&globs, "posts/welcome/index.qmd"));
        assert!(!item_matches(&globs, "posts/drafts/wip.qmd"));
    }

    #[test]
    fn no_positive_matches_nothing() {
        assert!(!item_matches(&[], "sub/p1.qmd"));
        let only_neg = vec![neg("sub/p2.qmd")];
        // resolve_content_globs injects the default positive before
        // matching; raw negation-only lists match nothing.
        assert!(!item_matches(&only_neg, "sub/p1.qmd"));
    }
}
