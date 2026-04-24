/*
 * project/index.rs
 * Copyright (c) 2026 Posit, PBC
 *
 * Cross-document index over Pass-1 DocumentProfiles.
 */

//! Cross-document index.
//!
//! Built from the `Vec<DocumentProfile>` that Pass 1 of
//! [`ProjectPipeline`](super::orchestrator::ProjectPipeline) produces
//! and handed, read-only, to Pass 2 and to project-level hooks
//! (`pre_render`, `post_render`).
//!
//! Phase-1 scope: holds profiles plus lookup helpers by source path
//! and by output href. The struct is intentionally minimal — later
//! phases (sidebar generate, cross-doc link rewriting) may add
//! specialized indices on top, but those live in their own modules and
//! are derived from this one.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::document_profile::DocumentProfile;

/// Read-only view of every document profile in a project.
///
/// Constructed by the driver after Pass 1, wrapped in an `Arc`, and
/// shared across per-file Pass-2 runs. The struct is not `Serialize` —
/// individual `DocumentProfile`s are; the index itself is rebuilt in
/// memory on every run (see `claude-notes/instructions/coding.md`
/// §"HashMap and Determinism": non-serialized lookup maps may use
/// `HashMap`).
#[derive(Debug, Clone, Default)]
pub struct ProjectIndex {
    profiles: Vec<DocumentProfile>,
    by_source_path: HashMap<PathBuf, usize>,
    by_output_href: HashMap<String, usize>,
}

impl ProjectIndex {
    /// Build an index from a freshly-collected vector of profiles.
    ///
    /// Profile insertion order is preserved for any consumer that
    /// iterates (`profiles()`). Duplicate source paths / output hrefs
    /// are silently overwritten in the lookup maps — the driver is
    /// responsible for filing a diagnostic and deciding which one wins
    /// (policy is deferred to a later phase).
    pub fn new(profiles: Vec<DocumentProfile>) -> Self {
        let mut by_source_path = HashMap::with_capacity(profiles.len());
        let mut by_output_href = HashMap::with_capacity(profiles.len());
        for (idx, profile) in profiles.iter().enumerate() {
            by_source_path.insert(profile.source_path.clone(), idx);
            by_output_href.insert(profile.output_href.clone(), idx);
        }
        Self {
            profiles,
            by_source_path,
            by_output_href,
        }
    }

    /// All profiles, in insertion order.
    pub fn profiles(&self) -> &[DocumentProfile] {
        &self.profiles
    }

    /// Look up a profile by project-relative source path.
    pub fn lookup_by_source(&self, path: &Path) -> Option<&DocumentProfile> {
        self.by_source_path
            .get(path)
            .and_then(|&i| self.profiles.get(i))
    }

    /// Look up a profile by output href.
    pub fn lookup_by_href(&self, href: &str) -> Option<&DocumentProfile> {
        self.by_output_href
            .get(href)
            .and_then(|&i| self.profiles.get(i))
    }

    /// True when no profiles were collected.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    /// Number of profiles held.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use pampa::toc::TocEntry;

    fn make_profile(source: &str, href: &str, title: &str) -> DocumentProfile {
        DocumentProfile {
            profile_version: crate::document_profile::DOCUMENT_PROFILE_VERSION,
            source_path: PathBuf::from(source),
            output_href: href.to_string(),
            format_id: "html".to_string(),
            title: Some(title.to_string()),
            subtitle: None,
            description: None,
            authors: Vec::new(),
            date: None,
            categories: Vec::new(),
            keywords: Vec::new(),
            image: None,
            draft: false,
            outline: Vec::<TocEntry>::new(),
        }
    }

    #[test]
    fn index_round_trips_profiles() {
        let profiles = vec![
            make_profile("index.qmd", "index.html", "Home"),
            make_profile("about.qmd", "about.html", "About"),
            make_profile("docs/api.qmd", "docs/api.html", "API"),
        ];
        let index = ProjectIndex::new(profiles.clone());

        assert_eq!(index.len(), 3);
        assert!(!index.is_empty());
        // profiles() preserves insertion order
        let titles: Vec<_> = index
            .profiles()
            .iter()
            .map(|p| p.title.as_deref().unwrap())
            .collect();
        assert_eq!(titles, vec!["Home", "About", "API"]);

        // lookup_by_source returns the right profile
        assert_eq!(
            index
                .lookup_by_source(Path::new("about.qmd"))
                .unwrap()
                .title
                .as_deref(),
            Some("About"),
        );
        // lookup_by_source handles nested paths
        assert_eq!(
            index
                .lookup_by_source(Path::new("docs/api.qmd"))
                .unwrap()
                .title
                .as_deref(),
            Some("API"),
        );

        // lookup_by_href
        assert_eq!(
            index.lookup_by_href("index.html").unwrap().title.as_deref(),
            Some("Home"),
        );
    }

    #[test]
    fn index_lookup_miss_returns_none() {
        let index = ProjectIndex::new(vec![make_profile("a.qmd", "a.html", "A")]);
        assert!(index.lookup_by_source(Path::new("nope.qmd")).is_none());
        assert!(index.lookup_by_href("nope.html").is_none());
    }

    #[test]
    fn empty_index_reports_empty() {
        let index = ProjectIndex::default();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
        assert!(index.profiles().is_empty());
        assert!(index.lookup_by_source(Path::new("anything.qmd")).is_none());
    }
}
