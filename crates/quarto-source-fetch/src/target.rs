//! Turning what the user typed into something fetchable.
//!
//! Ported from Quarto 1's `extension-host.ts`, with three of its
//! defects fixed rather than carried over. See
//! `claude-notes/plans/2026-07-28-q2-use-brand-command.md` §"Quarto 1
//! defects we are deliberately not porting" for the full analysis; the
//! short version is that Q1 hardcodes `main` as the default branch, and
//! *predicts* the name of the directory inside a GitHub archive from
//! the ref string — a prediction that is wrong for refs containing `/`
//! and for tags beginning with `v`.
//!
//! This module does not predict anything. It produces candidate URLs;
//! the archive's actual layout is read from the archive after
//! extraction (see [`crate::fetch::derive_archive_root`]).

use std::path::{Path, PathBuf};

use crate::error::FetchError;

/// Where a brand source is coming from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A path on disk: either a directory, or an archive file.
    Local(PathBuf),
    /// Something to download.
    Remote(RemoteTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTarget {
    /// URLs to try in order; the first that answers 200 wins.
    pub candidates: Vec<String>,
    /// A subdirectory *within* the archive to use as the source root.
    /// Already validated as a safe relative path.
    pub subdir: Option<PathBuf>,
    /// How to describe this source to the user in a trust prompt.
    pub description: String,
    /// A page the user can read before trusting it, when we know one.
    pub learn_more: Option<String>,
}

/// GitHub serves `.tar.gz` for every ref on every platform.
///
/// Quarto 1 requests `.zip` on Windows (`extension-host.ts:109`) purely
/// because Deno shells out to platform archive tools. Rust has no such
/// constraint, so one format is requested everywhere and the platform
/// stops being a variable. `.zip` support still exists for
/// user-supplied URLs and local files.
const GITHUB_ARCHIVE_EXT: &str = ".tar.gz";

/// Branches probed, in order, when the user gives no `@ref`.
///
/// Quarto 1 hardcodes `main` and reports "not found" for a repository
/// whose default branch is `master` — a message that points at the
/// wrong problem. Probing both costs one extra request in the rare
/// case and, more importantly, lets the failure message name the real
/// problem when neither exists.
const DEFAULT_BRANCHES: [&str; 2] = ["main", "master"];

/// Resolve user input into a fetchable target.
///
/// Accepted forms, in the order they are recognized:
///
/// 1. an existing path on disk (directory or archive file);
/// 2. an `http(s)://` URL, taken as a direct archive URL;
/// 3. `<org>/<repo>[/<subdir>][@<ref>]` on GitHub.
///
/// Checking the filesystem first matches Quarto 1 and means a local
/// directory named like `org/repo` is treated as the local thing it is.
pub fn resolve_target(input: &str) -> Result<Target, FetchError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(FetchError::UnrecognizedTarget {
            target: input.to_string(),
            reason: "it is empty",
        });
    }

    if !is_url(trimmed) {
        let path = Path::new(trimmed);
        if path.exists() {
            return Ok(Target::Local(path.to_path_buf()));
        }
    }

    if is_url(trimmed) {
        return Ok(Target::Remote(RemoteTarget {
            candidates: vec![trimmed.to_string()],
            subdir: None,
            description: trimmed.to_string(),
            learn_more: None,
        }));
    }

    parse_github(trimmed).map(Target::Remote)
}

fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Parse `<org>/<repo>[/<subdir>][@<ref>]`.
fn parse_github(input: &str) -> Result<RemoteTarget, FetchError> {
    let unrecognized = |reason: &'static str| FetchError::UnrecognizedTarget {
        target: input.to_string(),
        reason,
    };

    // Split the ref off the end. Splitting on the *last* `@` keeps a
    // ref that itself contains `@` working, and a subdirectory
    // containing `@` is far less likely than a ref that does.
    let (locator, git_ref) = match input.rsplit_once('@') {
        Some((locator, r)) if !r.is_empty() => (locator, Some(r)),
        Some(_) => return Err(unrecognized("it ends with `@` but names no ref")),
        None => (input, None),
    };

    let mut segments = locator.split('/');
    let org = segments.next().unwrap_or_default();
    let repo = segments.next().unwrap_or_default();
    let subdir: Vec<&str> = segments.filter(|s| !s.is_empty()).collect();

    if org.is_empty() || repo.is_empty() {
        return Err(unrecognized(
            "expected `<org>/<repo>`, a URL, or a path that exists",
        ));
    }
    for (label, part) in [("organization", org), ("repository", repo)] {
        if !part.chars().all(is_github_name_char) {
            return Err(match label {
                "organization" => unrecognized("the organization name has invalid characters"),
                _ => unrecognized("the repository name has invalid characters"),
            });
        }
    }

    let subdir = if subdir.is_empty() {
        None
    } else {
        // The subdirectory is joined onto an extracted archive root, so
        // it gets the same treatment as an archive entry name: a
        // `..` here would climb out of the extracted tree.
        Some(
            crate::archive::sanitize_relative_path(&subdir.join("/")).map_err(|_| {
                FetchError::UnrecognizedTarget {
                    target: input.to_string(),
                    reason: "the subdirectory is not a plain relative path",
                }
            })?,
        )
    };

    let candidates = match git_ref {
        // A ref could be either a tag or a branch, and GitHub serves
        // them from different paths. Try tag first: `@v1.2.0` is
        // overwhelmingly a tag, and a branch of the same name is rare.
        Some(r) => vec![
            github_archive_url(org, repo, "tags", r),
            github_archive_url(org, repo, "heads", r),
        ],
        None => DEFAULT_BRANCHES
            .iter()
            .map(|branch| github_archive_url(org, repo, "heads", branch))
            .collect(),
    };

    let description = match git_ref {
        Some(r) => format!("{org}/{repo}@{r} on GitHub"),
        None => format!("{org}/{repo} on GitHub"),
    };

    Ok(RemoteTarget {
        candidates,
        subdir,
        description,
        learn_more: Some(format!("https://github.com/{org}/{repo}")),
    })
}

fn is_github_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')
}

fn github_archive_url(org: &str, repo: &str, kind: &str, git_ref: &str) -> String {
    format!("https://github.com/{org}/{repo}/archive/refs/{kind}/{git_ref}{GITHUB_ARCHIVE_EXT}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn remote(input: &str) -> RemoteTarget {
        match resolve_target(input).unwrap() {
            Target::Remote(r) => r,
            other => panic!("expected a remote target for {input:?}, got {other:?}"),
        }
    }

    #[test]
    fn an_existing_directory_is_local() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_string_lossy().into_owned();
        assert_eq!(
            resolve_target(&path).unwrap(),
            Target::Local(PathBuf::from(&path))
        );
    }

    #[test]
    fn an_existing_file_is_local() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("brand.tar.gz");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(
            resolve_target(&file.to_string_lossy()).unwrap(),
            Target::Local(file)
        );
    }

    #[test]
    fn a_url_is_used_verbatim() {
        let r = remote("https://example.com/brand.zip");
        assert_eq!(r.candidates, ["https://example.com/brand.zip"]);
        assert!(r.subdir.is_none());
    }

    #[test]
    fn bare_org_repo_probes_main_then_master() {
        // Quarto 1 probes only `main`, so a master-default repository
        // reports "not found" (extension-host.ts:114).
        let r = remote("posit-dev/brand");
        assert_eq!(
            r.candidates,
            [
                "https://github.com/posit-dev/brand/archive/refs/heads/main.tar.gz",
                "https://github.com/posit-dev/brand/archive/refs/heads/master.tar.gz",
            ]
        );
        assert_eq!(
            r.learn_more.as_deref(),
            Some("https://github.com/posit-dev/brand")
        );
    }

    #[test]
    fn a_ref_probes_tag_then_branch() {
        let r = remote("org/repo@v1.2.0");
        assert_eq!(
            r.candidates,
            [
                "https://github.com/org/repo/archive/refs/tags/v1.2.0.tar.gz",
                "https://github.com/org/repo/archive/refs/heads/v1.2.0.tar.gz",
            ]
        );
    }

    #[test]
    fn a_ref_containing_a_slash_is_passed_through_untouched() {
        // Q1 would go on to compute an archive subdirectory of
        // `repo-feature/foo` from this — a two-segment path that no
        // single archive root can match. We build no such prediction.
        let r = remote("org/repo@feature/foo");
        assert_eq!(
            r.candidates[1],
            "https://github.com/org/repo/archive/refs/heads/feature/foo.tar.gz"
        );
    }

    #[test]
    fn a_tag_starting_with_v_is_not_mangled() {
        // Q1's `tagSubDirectory` strips a leading `v` from *any* tag, so
        // `valid-release` becomes `alid-release`. Nothing here rewrites
        // the ref, because nothing here needs to guess a directory name.
        let r = remote("org/repo@valid-release");
        // Note: `"valid-release".contains("alid-release")` is true, so
        // asserting on the bare substring would pass no matter what.
        // Assert on the full path segment the URL would carry.
        assert!(
            r.candidates
                .iter()
                .all(|c| c.ends_with("/valid-release.tar.gz")),
            "the ref must reach the URL unmodified: {:?}",
            r.candidates
        );
    }

    #[test]
    fn a_subdirectory_is_captured() {
        let r = remote("org/repo/brands/dark@v1");
        assert_eq!(r.subdir, Some(PathBuf::from("brands").join("dark")));
        assert!(r.candidates[0].contains("/refs/tags/v1.tar.gz"));
    }

    #[test]
    fn a_traversing_subdirectory_is_refused() {
        // The subdir is joined onto the extracted tree, so it is an
        // escape vector exactly like an archive entry name.
        assert!(resolve_target("org/repo/../../etc").is_err());
    }

    #[test]
    fn nonsense_targets_are_refused_with_a_reason() {
        for input in ["", "   ", "notarepo", "org/", "/repo", "org/repo@"] {
            let err = resolve_target(input)
                .err()
                .unwrap_or_else(|| panic!("{input:?} should be refused"));
            assert!(
                matches!(err, FetchError::UnrecognizedTarget { .. }),
                "{input:?} got: {err}"
            );
        }
    }

    #[test]
    fn invalid_name_characters_are_refused() {
        assert!(resolve_target("org name/repo").is_err());
        assert!(resolve_target("org/re po").is_err());
    }
}
