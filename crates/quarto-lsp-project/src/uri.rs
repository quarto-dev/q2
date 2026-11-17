//! Document URIs and the `file:` scheme.
//!
//! Only this module turns a URI into a path. Everything else keys documents
//! by [`DocUri`] at the edges and by project-relative path inside a session
//! (plan: "Identity: sessions, paths, and URIs"). Attached mode (F3) adds
//! `automerge:` URLs here.

use std::path::{Path, PathBuf};

/// A document URI exactly as the client sent it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocUri(String);

impl DocUri {
    pub fn new(uri: impl Into<String>) -> Self {
        Self(uri.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The local path of a `file:` URI; `None` for any other scheme (and
    /// always on wasm32, where there is no local file system).
    pub fn to_file_path(&self) -> Option<PathBuf> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let url = url::Url::parse(&self.0).ok()?;
            if url.scheme() != "file" {
                return None;
            }
            url.to_file_path().ok()
        }
        #[cfg(target_arch = "wasm32")]
        {
            None
        }
    }

    /// The `file:` URI of an absolute local path.
    pub fn from_file_path(path: &Path) -> Option<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            url::Url::from_file_path(path)
                .ok()
                .map(|u| Self(u.to_string()))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            None
        }
    }

    /// A short name for a document that has no path (`untitled:Untitled-1`
    /// → `Untitled-1`), used as its file name in a synthetic session.
    pub(crate) fn display_name(&self) -> String {
        let tail = self
            .0
            .rsplit(['/', ':'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("untitled");
        tail.to_string()
    }
}

impl std::fmt::Display for DocUri {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn file_uris_round_trip_through_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a dir").join("doc 1.qmd");
        let uri = DocUri::from_file_path(&path).unwrap();
        assert!(uri.as_str().starts_with("file://"), "{uri}");
        assert!(
            uri.as_str().contains("%20"),
            "spaces are percent-encoded: {uri}"
        );
        assert_eq!(uri.to_file_path().unwrap(), path);
    }

    #[test]
    fn other_schemes_have_no_path() {
        assert_eq!(DocUri::new("untitled:Untitled-1").to_file_path(), None);
        assert_eq!(DocUri::new("automerge:abc").to_file_path(), None);
        assert_eq!(
            DocUri::new("untitled:Untitled-1").display_name(),
            "Untitled-1"
        );
    }
}
