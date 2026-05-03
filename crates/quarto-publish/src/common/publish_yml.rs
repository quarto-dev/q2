//! `_publish.yml` reader (Q1-compatible).
//!
//! Q1 schema (preserved for forward compatibility):
//!
//! ```yaml
//! - source: index.qmd
//!   gh-pages:
//!     - id: gh-pages
//!       url: https://example.com/
//!   netlify:
//!     - id: 01234567-89ab-cdef-0123-456789abcdef
//!       url: https://example.netlify.app
//! - source: other.qmd
//!   gh-pages:
//!     - id: gh-pages
//!       url: https://other.example.com/
//! ```
//!
//! The file lives next to the project root (or alongside the
//! source file for single-doc renders). Each top-level array item
//! is keyed by `source` and has one entry per provider.
//!
//! Phase 1 ships only the *reader*. The writer comes when the first
//! provider that needs it (Quarto Pub, Netlify) lands — gh-pages
//! detects re-publish from git state, not from `_publish.yml`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::common::errors::unable_to_publish;
use crate::types::{PublishError, PublishRecord};

const FILE_NAMES: &[&str] = &["_publish.yml", "_publish.yaml"];

/// Aggregated publish history for one source file.
#[derive(Debug, Clone, Default)]
pub struct PublishDeployments {
    /// Directory the file was found in (or would-be found in).
    pub dir: PathBuf,
    /// Source basename the records belong to.
    pub source: String,
    /// Provider name → list of historical publish records.
    pub records: HashMap<String, Vec<PublishRecord>>,
}

/// Locate `_publish.yml` (or `_publish.yaml`) next to `source`.
///
/// `source` can be a directory (we look in it directly) or a file
/// (we look in its parent). Returns `None` if no such file exists.
pub fn locate_publish_yml(source: &Path) -> Option<PathBuf> {
    let dir = if source.is_dir() {
        source.to_path_buf()
    } else {
        source.parent().unwrap_or(Path::new(".")).to_path_buf()
    };
    for name in FILE_NAMES {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Read all publish deployments for `source` from the matching
/// `_publish.yml`. Returns an empty `PublishDeployments` (with
/// resolved `dir` and `source`) when no file or no entry exists.
pub fn read_publish_deployments(source: &Path) -> Result<PublishDeployments, PublishError> {
    let (dir, source_basename) = resolve_deployment_source(source);
    let file = match locate_publish_yml(source) {
        Some(p) => p,
        None => {
            return Ok(PublishDeployments {
                dir,
                source: source_basename,
                records: HashMap::new(),
            });
        }
    };

    let bytes = std::fs::read(&file).map_err(|e| {
        unable_to_publish("publish", format!("could not read {}: {e}", file.display()))
    })?;
    let raw: Vec<RawSourceEntry> = serde_yaml::from_slice(&bytes).map_err(|e| {
        unable_to_publish(
            "publish",
            format!(
                "{} is not a valid _publish.yml (expected an array of mappings): {e}",
                file.display()
            ),
        )
    })?;

    let mut records: HashMap<String, Vec<PublishRecord>> = HashMap::new();
    if let Some(entry) = raw.into_iter().find(|e| e.source == source_basename) {
        for (provider, recs) in entry.providers {
            records.insert(provider, recs);
        }
    }

    Ok(PublishDeployments {
        dir,
        source: source_basename,
        records,
    })
}

/// Split `source` into `(dir, basename)`. For directory-shaped
/// sources, the basename is the directory's own name (Q1 parity).
fn resolve_deployment_source(source: &Path) -> (PathBuf, String) {
    if source.is_dir() {
        let name = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        (source.to_path_buf(), name)
    } else {
        let dir = source.parent().unwrap_or(Path::new(".")).to_path_buf();
        let name = source
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        (dir, name)
    }
}

/// Internal: one entry under the top-level array.
#[derive(Debug, Deserialize)]
struct RawSourceEntry {
    source: String,
    /// Everything else is provider-keyed: `gh-pages: [...]`,
    /// `netlify: [...]`, etc. Captured via `flatten` into a free
    /// map so unknown providers round-trip unchanged.
    #[serde(flatten)]
    providers: HashMap<String, Vec<PublishRecord>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, contents: &str) {
        fs::write(dir.join(name), contents).unwrap();
    }

    // ── locate_publish_yml ──────────────────────────────────────

    #[test]
    fn locate_finds_publish_yml_in_directory() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "_publish.yml", "[]\n");
        assert_eq!(
            locate_publish_yml(temp.path()).unwrap(),
            temp.path().join("_publish.yml")
        );
    }

    #[test]
    fn locate_finds_publish_yaml_extension() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "_publish.yaml", "[]\n");
        assert_eq!(
            locate_publish_yml(temp.path()).unwrap(),
            temp.path().join("_publish.yaml")
        );
    }

    #[test]
    fn locate_returns_none_when_absent() {
        let temp = TempDir::new().unwrap();
        assert!(locate_publish_yml(temp.path()).is_none());
    }

    #[test]
    fn locate_uses_file_parent_when_source_is_a_file() {
        let temp = TempDir::new().unwrap();
        write(temp.path(), "_publish.yml", "[]\n");
        let qmd = temp.path().join("index.qmd");
        write(temp.path(), "index.qmd", "# hi");
        assert_eq!(
            locate_publish_yml(&qmd).unwrap(),
            temp.path().join("_publish.yml")
        );
    }

    // ── read_publish_deployments ────────────────────────────────

    #[test]
    fn read_returns_empty_records_when_file_absent() {
        let temp = TempDir::new().unwrap();
        let qmd = temp.path().join("index.qmd");
        fs::write(&qmd, "# hi").unwrap();
        let d = read_publish_deployments(&qmd).unwrap();
        assert_eq!(d.source, "index.qmd");
        assert!(d.records.is_empty());
    }

    #[test]
    fn read_parses_q1_shape_with_gh_pages_and_netlify() {
        let temp = TempDir::new().unwrap();
        let yml = "- source: index.qmd
  gh-pages:
    - id: gh-pages
      url: https://example.com/
  netlify:
    - id: 01234567-89ab-cdef-0123-456789abcdef
      url: https://example.netlify.app
- source: other.qmd
  gh-pages:
    - id: gh-pages
      url: https://other.example.com/
";
        write(temp.path(), "_publish.yml", yml);
        let qmd = temp.path().join("index.qmd");
        fs::write(&qmd, "# hi").unwrap();
        let d = read_publish_deployments(&qmd).unwrap();
        assert_eq!(d.source, "index.qmd");
        assert_eq!(d.records.len(), 2);

        let gh = &d.records["gh-pages"];
        assert_eq!(gh.len(), 1);
        assert_eq!(gh[0].id, "gh-pages");
        assert_eq!(gh[0].url.as_deref(), Some("https://example.com/"));

        let nf = &d.records["netlify"];
        assert_eq!(nf.len(), 1);
        assert_eq!(nf[0].id, "01234567-89ab-cdef-0123-456789abcdef");
    }

    #[test]
    fn read_isolates_records_to_the_named_source() {
        let temp = TempDir::new().unwrap();
        let yml = "- source: index.qmd
  gh-pages:
    - id: gh-pages
      url: https://index.example.com/
- source: other.qmd
  gh-pages:
    - id: gh-pages
      url: https://other.example.com/
";
        write(temp.path(), "_publish.yml", yml);

        let other = temp.path().join("other.qmd");
        fs::write(&other, "# hi").unwrap();
        let d = read_publish_deployments(&other).unwrap();
        let gh = &d.records["gh-pages"];
        assert_eq!(gh[0].url.as_deref(), Some("https://other.example.com/"));
    }

    #[test]
    fn read_returns_empty_when_named_source_not_in_yml() {
        let temp = TempDir::new().unwrap();
        let yml = "- source: index.qmd
  gh-pages:
    - id: gh-pages
      url: https://example.com/
";
        write(temp.path(), "_publish.yml", yml);
        let other = temp.path().join("missing.qmd");
        fs::write(&other, "x").unwrap();
        let d = read_publish_deployments(&other).unwrap();
        assert!(d.records.is_empty());
    }

    #[test]
    fn read_errors_on_malformed_yaml() {
        let temp = TempDir::new().unwrap();
        // A mapping at top-level is not the array Q1 expects.
        write(temp.path(), "_publish.yml", "key: value\n");
        let qmd = temp.path().join("index.qmd");
        fs::write(&qmd, "x").unwrap();
        let err = read_publish_deployments(&qmd).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not a valid _publish.yml"), "got: {msg}");
    }

    #[test]
    fn read_returns_unknown_providers_unchanged() {
        // Forward compatibility: if a future provider lands and a
        // user has its records, we should round-trip them as
        // structured PublishRecords without erroring.
        let temp = TempDir::new().unwrap();
        let yml = "- source: index.qmd
  some-future-provider:
    - id: abc-123
      url: https://example.com/
";
        write(temp.path(), "_publish.yml", yml);
        let qmd = temp.path().join("index.qmd");
        fs::write(&qmd, "x").unwrap();
        let d = read_publish_deployments(&qmd).unwrap();
        assert!(d.records.contains_key("some-future-provider"));
    }

    #[test]
    fn read_directory_source_uses_directory_basename() {
        let temp = TempDir::new().unwrap();
        let project = temp.path().join("my-project");
        fs::create_dir(&project).unwrap();
        let yml = "- source: my-project
  gh-pages:
    - id: gh-pages
      url: https://example.com/
";
        write(&project, "_publish.yml", yml);
        let d = read_publish_deployments(&project).unwrap();
        assert_eq!(d.source, "my-project");
        assert_eq!(d.records.len(), 1);
    }
}
