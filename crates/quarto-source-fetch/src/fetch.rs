//! Downloading a source archive and turning it into a directory.
//!
//! The HTTP client sits behind a trait for the same reason
//! `quarto-publish` puts one behind `PublishHost::http_get`: the
//! candidate-ordering and archive-layout logic is worth testing without
//! a network, and the real client is worth testing without inventing a
//! fake HTTP stack. Both happen — see the crate's integration tests.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::archive::extract_into;
use crate::error::FetchError;
use crate::limits::ExtractLimits;
use crate::target::{RemoteTarget, Target};

/// One HTTP GET, streamed to a file.
pub trait SourceFetch {
    /// Fetch `url` into `dest`.
    ///
    /// Returns the HTTP status. The body is written only for a 2xx —
    /// a 404 body is HTML, and writing it would leave a file that looks
    /// like a failed download but smells like an archive.
    ///
    /// Implementations must stop reading past `limits.max_download_bytes`
    /// and return [`FetchError::DownloadTooLarge`]: the response's
    /// declared length, when it has one, comes from the server.
    fn get_to_file(
        &self,
        url: &str,
        dest: &Path,
        limits: &ExtractLimits,
    ) -> Result<u16, FetchError>;
}

/// The real client, backed by `ureq`.
///
/// Sync inside a sync command, like `quarto-publish`'s native host: the
/// alternative is tokio + reqwest for what is at most three requests.
pub struct UreqFetch;

impl SourceFetch for UreqFetch {
    fn get_to_file(
        &self,
        url: &str,
        dest: &Path,
        limits: &ExtractLimits,
    ) -> Result<u16, FetchError> {
        // Refuse an https→http downgrade: if the user asked for https,
        // a redirect must not quietly drop to plaintext. An explicitly
        // `http://` target is the user's own choice and is left alone.
        let https_only = url.starts_with("https://");

        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(limits.request_timeout))
            .https_only(https_only)
            .max_redirects(5)
            // Turn "too many redirects" into an error rather than
            // handing back the last 3xx as if it were the answer.
            .max_redirects_will_error(true)
            .build()
            .into();

        let response = match agent.get(url).call() {
            Ok(r) => r,
            Err(ureq::Error::StatusCode(code)) => return Ok(code),
            Err(e) => {
                return Err(FetchError::Network {
                    url: url.to_string(),
                    message: e.to_string(),
                });
            }
        };

        let status: u16 = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Ok(status);
        }

        let mut body = response.into_body().into_reader();
        stream_to_file(&mut body, dest, limits)?;
        Ok(status)
    }
}

/// Copy `reader` into `dest`, aborting past the download ceiling.
fn stream_to_file(
    reader: &mut impl Read,
    dest: &Path,
    limits: &ExtractLimits,
) -> Result<(), FetchError> {
    let mut file = std::fs::File::create(dest)
        .map_err(|e| FetchError::io(format!("create {}", dest.display()), e))?;
    let mut total: u64 = 0;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| FetchError::io("read response body", e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limits.max_download_bytes {
            // Drop the partial file: leaving it invites a later step to
            // treat a truncated archive as a real one.
            let _ = std::fs::remove_file(dest);
            return Err(FetchError::DownloadTooLarge {
                limit: limits.max_download_bytes,
            });
        }
        file.write_all(&buf[..n])
            .map_err(|e| FetchError::io(format!("write {}", dest.display()), e))?;
    }
    Ok(())
}

/// Materialize `target` as a directory containing its source files.
///
/// `work_dir` is caller-owned scratch space; everything this function
/// writes goes under it, and the returned path points inside it (except
/// for a local-directory target, which is returned as-is and never
/// copied). The caller inspects the result and decides what, if
/// anything, to copy into the user's project.
pub fn fetch_into(
    target: &Target,
    work_dir: &Path,
    fetcher: &dyn SourceFetch,
    limits: &ExtractLimits,
) -> Result<PathBuf, FetchError> {
    match target {
        Target::Local(path) if path.is_dir() => Ok(path.clone()),
        Target::Local(path) => {
            let extracted = work_dir.join("extracted");
            std::fs::create_dir_all(&extracted)
                .map_err(|e| FetchError::io(format!("create {}", extracted.display()), e))?;
            extract_into(path, &extracted, limits)?;
            derive_archive_root(&extracted, None)
        }
        Target::Remote(remote) => {
            let download = work_dir.join("download");
            fetch_first_available(remote, &download, fetcher, limits)?;

            let extracted = work_dir.join("extracted");
            std::fs::create_dir_all(&extracted)
                .map_err(|e| FetchError::io(format!("create {}", extracted.display()), e))?;
            extract_into(&download, &extracted, limits)?;
            derive_archive_root(&extracted, remote.subdir.as_deref())
        }
    }
}

/// Try each candidate URL in order; the first 2xx wins.
fn fetch_first_available(
    remote: &RemoteTarget,
    dest: &Path,
    fetcher: &dyn SourceFetch,
    limits: &ExtractLimits,
) -> Result<(), FetchError> {
    let mut attempts = Vec::new();
    for url in &remote.candidates {
        match fetcher.get_to_file(url, dest, limits) {
            Ok(status) if (200..300).contains(&status) => return Ok(()),
            Ok(status) => attempts.push(format!("{url} → HTTP {status}")),
            // A network-level failure on one candidate should not mask
            // a later candidate that works, but it must still be
            // reported if nothing works — otherwise a DNS failure looks
            // identical to a missing repository.
            Err(FetchError::Network { url, message }) => {
                attempts.push(format!("{url} → {message}"));
            }
            Err(other) => return Err(other),
        }
    }

    Err(FetchError::NotFound {
        description: remote.description.clone(),
        detail: format!(
            "tried {} location(s): {}",
            attempts.len(),
            attempts.join("; ")
        ),
    })
}

/// Find the directory an extracted archive's content actually lives in,
/// then descend into `subdir` if one was requested.
///
/// **Derived, never predicted.** Quarto 1 computes an expected
/// directory name from the ref (`<repo>-<ref>`), which is wrong for any
/// ref containing `/` and for tags beginning with `v` — and which
/// requires modeling GitHub's undocumented naming rules in the first
/// place. The archive is in hand by the time this runs, so the question
/// can simply be answered instead of guessed: a tarball whose entire
/// content sits under one top-level directory has that directory as its
/// root; anything else is its own root.
pub fn derive_archive_root(extracted: &Path, subdir: Option<&Path>) -> Result<PathBuf, FetchError> {
    let mut entries = Vec::new();
    let read = std::fs::read_dir(extracted)
        .map_err(|e| FetchError::io(format!("read {}", extracted.display()), e))?;
    for entry in read {
        let entry = entry.map_err(|e| FetchError::io("read extracted entry", e))?;
        entries.push(entry.path());
    }

    let root = match entries.as_slice() {
        [only] if only.is_dir() => only.clone(),
        _ => extracted.to_path_buf(),
    };

    let Some(subdir) = subdir else {
        return Ok(root);
    };
    let target = root.join(subdir);
    if !target.is_dir() {
        return Err(FetchError::SubdirectoryNotFound {
            subdir: subdir.display().to_string(),
            available: list_directories(&root),
        });
    }
    Ok(target)
}

/// Directory names directly under `dir`, for a "did you mean" hint.
fn list_directories(dir: &Path) -> String {
    let Ok(read) = std::fs::read_dir(dir) else {
        return "(could not list the archive contents)".to_string();
    };
    let mut names: Vec<String> = read
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    if names.is_empty() {
        "the archive has no subdirectories".to_string()
    } else {
        format!("available: {}", names.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn touch_dir(base: &Path, rel: &str) -> PathBuf {
        let p = base.join(rel);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_lone_top_level_directory_is_the_root() {
        let tmp = TempDir::new().unwrap();
        let inner = touch_dir(tmp.path(), "repo-main");
        std::fs::write(inner.join("_brand.yml"), "color:\n").unwrap();

        assert_eq!(derive_archive_root(tmp.path(), None).unwrap(), inner);
    }

    #[test]
    fn a_ref_with_a_slash_needs_no_special_handling() {
        // GitHub names this directory however it likes; we read it.
        let tmp = TempDir::new().unwrap();
        let inner = touch_dir(tmp.path(), "repo-feature-foo");
        assert_eq!(derive_archive_root(tmp.path(), None).unwrap(), inner);
    }

    #[test]
    fn a_v_prefixed_tag_needs_no_special_handling() {
        let tmp = TempDir::new().unwrap();
        let inner = touch_dir(tmp.path(), "repo-valid-release");
        assert_eq!(derive_archive_root(tmp.path(), None).unwrap(), inner);
    }

    #[test]
    fn multiple_top_level_entries_keep_the_extraction_dir_as_root() {
        let tmp = TempDir::new().unwrap();
        touch_dir(tmp.path(), "a");
        touch_dir(tmp.path(), "b");
        assert_eq!(derive_archive_root(tmp.path(), None).unwrap(), tmp.path());
    }

    #[test]
    fn a_lone_top_level_file_keeps_the_extraction_dir_as_root() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("_brand.yml"), "color:\n").unwrap();
        assert_eq!(derive_archive_root(tmp.path(), None).unwrap(), tmp.path());
    }

    #[test]
    fn a_subdirectory_is_applied_beneath_the_derived_root() {
        let tmp = TempDir::new().unwrap();
        let inner = touch_dir(tmp.path(), "repo-main");
        let sub = touch_dir(&inner, "brands/dark");

        assert_eq!(
            derive_archive_root(tmp.path(), Some(Path::new("brands/dark"))).unwrap(),
            sub
        );
    }

    #[test]
    fn a_missing_subdirectory_errors_with_what_is_available() {
        let tmp = TempDir::new().unwrap();
        let inner = touch_dir(tmp.path(), "repo-main");
        touch_dir(&inner, "brands");
        touch_dir(&inner, "docs");

        let err = derive_archive_root(tmp.path(), Some(Path::new("nope")))
            .expect_err("a missing subdirectory must be reported");
        let msg = err.to_string();
        assert!(msg.contains("nope"), "{msg}");
        assert!(msg.contains("brands") && msg.contains("docs"), "{msg}");
    }
}
