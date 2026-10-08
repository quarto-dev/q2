//! Per-request output directories for `q2 preview` renders (epic E9).
//!
//! A typst render must write inside the discovered project: typst refuses a
//! source outside its `--root` (`Q-21-3`). So a preview render goes to
//! `<project dir>/.quarto/preview-out/<uuid>/`, where `<project dir>` is
//! what [`ProjectContext::discover`] reports for the document (the document's
//! own directory when there is no `_quarto.yml`).
//!
//! - [`PreviewOutDir`] creates the directory and removes it on drop. A caller
//!   reads the output bytes first and drops the guard after, inside its
//!   blocking closure, so success, error and abort all clean up.
//! - [`sweep_preview_out`] removes directories a crashed or killed server
//!   left behind, by age, so a second `q2 preview` on the same project keeps
//!   its live directories.
//! - `.quarto/preview-out/` gets a self-written `.gitignore` of `*`.
//!
//! Rejected: widening typst's `--root`, which would let raw `{=typst}` `#read`
//! reach far beyond the project.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use quarto_system_runtime::SystemRuntime;

use crate::error::{QuartoError, Result};
use crate::project::ProjectContext;

/// Where request directories live, relative to the discovered project dir.
pub const PREVIEW_OUT_REL: &str = ".quarto/preview-out";

/// Directories older than this are swept at server start. Far above any
/// render's wall time, so a live second server's directories survive.
pub const SWEEP_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// A request directory under a project's `.quarto/preview-out/`, removed
/// (best effort) when dropped.
pub struct PreviewOutDir {
    path: PathBuf,
    project: ProjectContext,
}

impl PreviewOutDir {
    /// Create `<discovered project dir>/.quarto/preview-out/<uuid>/` for the
    /// document at `abs_path`.
    ///
    /// The discovered [`ProjectContext`] is kept ([`Self::project`]) so the
    /// caller renders with it: the directory and typst's `--root` cannot
    /// disagree, and discovery runs once.
    ///
    /// # Errors
    ///
    /// Project discovery fails, or the project is not writable (there is no
    /// fallback location: typst needs the directory inside its root).
    pub fn create(abs_path: &Path, runtime: &dyn SystemRuntime) -> Result<Self> {
        let project = ProjectContext::discover(abs_path, runtime)?;
        let root = project.dir.join(PREVIEW_OUT_REL);
        let path = root.join(uuid::Uuid::new_v4().to_string());
        let unwritable = |e: std::io::Error| {
            QuartoError::other(format!(
                "Cannot create the preview output directory {}: {e}. \
                 PDF preview and typst downloads need a writable project directory.",
                path.display()
            ))
        };
        std::fs::create_dir_all(&path).map_err(unwritable)?;
        // Ignore everything under `.quarto/preview-out`, including this file.
        let gitignore = root.join(".gitignore");
        if !gitignore.exists() {
            std::fs::write(&gitignore, "*\n").map_err(unwritable)?;
        }
        Ok(Self { path, project })
    }

    /// The request directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The project discovered for the document.
    pub fn project(&self) -> &ProjectContext {
        &self.project
    }
}

impl Drop for PreviewOutDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Directory names the sweep never descends into.
const SKIPPED_DIRS: &[&str] = &[".git", "node_modules", "target"];

/// Bound on the sweep's walk below the hub root.
const SWEEP_MAX_DEPTH: usize = 12;

/// Remove request directories older than `max_age` from the one
/// `.quarto/preview-out` of the project at `project_dir` (no walk; for
/// single-file mode, where the root may be a large unrelated directory).
/// Returns how many were removed.
pub fn sweep_project_preview_out(project_dir: &Path, max_age: Duration) -> usize {
    sweep_dir(
        &project_dir.join(PREVIEW_OUT_REL),
        SystemTime::now(),
        max_age,
    )
}

fn sweep_dir(out_dir: &Path, now: SystemTime, max_age: Duration) -> usize {
    let Ok(children) = std::fs::read_dir(out_dir) else {
        return 0;
    };
    let mut removed = 0;
    for child in children.flatten() {
        let path = child.path();
        if !path.is_dir() {
            continue;
        }
        let age = child
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok());
        if age.is_some_and(|a| a >= max_age) && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// Remove request directories older than `max_age` from every
/// `.quarto/preview-out` under `hub_root`, nested discovered projects
/// included. Returns how many were removed. Best effort: unreadable entries
/// are skipped.
pub fn sweep_preview_out(hub_root: &Path, max_age: Duration) -> usize {
    let now = SystemTime::now();
    let mut removed = 0;
    let walker = walkdir::WalkDir::new(hub_root)
        .max_depth(SWEEP_MAX_DEPTH)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && e.depth() > 0
                && e.file_name()
                    .to_str()
                    .is_some_and(|n| SKIPPED_DIRS.contains(&n)))
        });
    for entry in walker.flatten() {
        let is_out = entry.file_type().is_dir()
            && entry.file_name() == "preview-out"
            && entry.path().parent().and_then(|p| p.file_name()) == Some(".quarto".as_ref());
        if is_out {
            removed += sweep_dir(entry.path(), now, max_age);
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use quarto_system_runtime::NativeRuntime;
    use tempfile::TempDir;

    fn project(root: &Path, yml: bool) -> PathBuf {
        if yml {
            std::fs::write(root.join("_quarto.yml"), "project:\n  title: x\n").unwrap();
        }
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let doc = root.join("sub/a.qmd");
        std::fs::write(&doc, "# A\n").unwrap();
        doc
    }

    #[test]
    fn creates_under_the_discovered_project_and_cleans_up_on_drop() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let doc = project(&root, true);
        let out = PreviewOutDir::create(&doc, &NativeRuntime::new()).unwrap();
        let dir = out.path().to_path_buf();
        assert!(dir.starts_with(root.join(PREVIEW_OUT_REL)), "{dir:?}");
        assert!(dir.is_dir());
        assert_eq!(out.project().dir, root);
        assert_eq!(
            std::fs::read_to_string(root.join(PREVIEW_OUT_REL).join(".gitignore")).unwrap(),
            "*\n"
        );
        drop(out);
        assert!(!dir.exists());
        // The ignore file stays for the next request.
        assert!(root.join(PREVIEW_OUT_REL).join(".gitignore").is_file());
    }

    #[test]
    fn a_project_without_a_config_file_uses_the_document_directory() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let doc = project(&root, false);
        let out = PreviewOutDir::create(&doc, &NativeRuntime::new()).unwrap();
        assert!(
            out.path()
                .starts_with(root.join("sub").join(PREVIEW_OUT_REL))
        );
    }

    #[test]
    fn sweep_removes_only_old_directories_including_nested_projects() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let old = root.join(PREVIEW_OUT_REL).join("old");
        let nested = root.join("deep/proj").join(PREVIEW_OUT_REL).join("nested");
        let skipped = root
            .join("node_modules/x")
            .join(PREVIEW_OUT_REL)
            .join("kept");
        for d in [&old, &nested, &skipped] {
            std::fs::create_dir_all(d).unwrap();
        }
        // Nothing is older than an hour yet.
        assert_eq!(sweep_preview_out(&root, SWEEP_MAX_AGE), 0);
        assert!(old.is_dir() && nested.is_dir());
        // A zero cutoff makes everything old, except what the walk skips.
        assert_eq!(sweep_preview_out(&root, Duration::ZERO), 2);
        assert!(!old.exists() && !nested.exists());
        assert!(skipped.is_dir());
    }

    #[test]
    fn single_project_sweep_does_not_walk() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let here = root.join(PREVIEW_OUT_REL).join("a");
        let nested = root.join("sub").join(PREVIEW_OUT_REL).join("b");
        for d in [&here, &nested] {
            std::fs::create_dir_all(d).unwrap();
        }
        assert_eq!(sweep_project_preview_out(&root, Duration::ZERO), 1);
        assert!(!here.exists() && nested.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_project_reports_a_clear_error() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let doc = project(&root, true);
        let ro = std::fs::Permissions::from_mode(0o555);
        std::fs::set_permissions(&root, ro).unwrap();
        // Root can write anywhere; skip when permissions are not enforced.
        let enforced = std::fs::create_dir(root.join("probe")).is_err();
        let result = PreviewOutDir::create(&doc, &NativeRuntime::new());
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !enforced {
            return;
        }
        let err = result
            .err()
            .expect("read-only project must fail")
            .to_string();
        assert!(err.contains("writable project"), "{err}");
    }
}
