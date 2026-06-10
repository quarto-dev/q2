/*
 * vfs.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * In-memory virtual filesystem.
 *
 * Compiled on every target. The primary consumer is `WasmRuntime`
 * (browser environments, where there is no real filesystem), but the
 * type itself is pure `std` — native builds use it for tests and for
 * perf-harness drivers that exercise the same flush code paths the
 * WASM hub-client runs (bd-q3bxnq2e).
 */

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::traits::{RuntimeError, RuntimeResult};

/// Helper function to create a "not found" error.
pub(crate) fn not_found_error(path: &Path) -> RuntimeError {
    RuntimeError::Io(io::Error::new(
        io::ErrorKind::NotFound,
        format!("Path not found: {}", path.display()),
    ))
}

/// Write-path counters for the VFS (bd-q3bxnq2e). Printed on Drop when
/// `QUARTO_PERF_STATS=1` (gauge `perf.vfs-write`). Free when unused.
///
/// `skipped_writes` / `bytes_skipped` count writes elided by
/// change-detection (byte-identical content already present); they stay
/// zero until that path exists, but are part of the stats shape from the
/// start so before/after measurements share one output format.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VfsWriteStats {
    /// Number of `add_file` calls that inserted or replaced content.
    pub writes: usize,
    /// Total bytes inserted across all performed writes.
    pub bytes_written: usize,
    /// Number of writes skipped because identical content was present.
    pub skipped_writes: usize,
    /// Total bytes of skipped (byte-identical) writes.
    pub bytes_skipped: usize,
}

/// Virtual filesystem for WASM environments.
///
/// This provides an in-memory filesystem that can be pre-populated with
/// project files from automerge documents or other sources.
///
/// The VFS supports:
/// - Files with arbitrary byte content
/// - Directory structure (automatically created when files are added)
/// - Standard operations: read, write, remove, list, copy
///
/// Thread safety: Uses RwLock to satisfy Send + Sync trait bounds.
/// In practice, WASM is single-threaded so this is never contended.
#[derive(Debug, Default)]
pub struct VirtualFileSystem {
    /// File contents, keyed by normalized absolute path
    files: HashMap<PathBuf, Vec<u8>>,
    /// Directory entries (automatically includes parents of all files)
    directories: HashSet<PathBuf>,
    /// Project root directory (default working directory)
    project_root: PathBuf,
    /// Write-path diagnostic counters (bd-q3bxnq2e).
    stats: VfsWriteStats,
}

impl VirtualFileSystem {
    /// Create a new empty virtual filesystem.
    pub fn new() -> Self {
        let mut vfs = Self {
            files: HashMap::new(),
            directories: HashSet::new(),
            project_root: PathBuf::from("/project"),
            stats: VfsWriteStats::default(),
        };
        // Create the root directory
        vfs.directories.insert(PathBuf::from("/"));
        vfs.directories.insert(PathBuf::from("/project"));
        vfs
    }

    /// Create VFS with a custom project root.
    pub fn with_project_root(project_root: PathBuf) -> Self {
        let mut vfs = Self {
            files: HashMap::new(),
            directories: HashSet::new(),
            project_root: project_root.clone(),
            stats: VfsWriteStats::default(),
        };
        // Create the root directory and project root
        vfs.directories.insert(PathBuf::from("/"));
        vfs.add_directory_and_parents(&project_root);
        vfs
    }

    /// Add a file to the virtual filesystem.
    ///
    /// This will automatically create all parent directories.
    pub fn add_file(&mut self, path: &Path, contents: Vec<u8>) {
        let normalized = self.normalize_path(path);
        // Create parent directories
        if let Some(parent) = normalized.parent() {
            self.add_directory_and_parents(parent);
        }
        self.stats.writes += 1;
        self.stats.bytes_written += contents.len();
        self.files.insert(normalized, contents);
    }

    /// Snapshot of the write-path counters (bd-q3bxnq2e). Counters are
    /// cumulative over the VFS's lifetime; callers wanting per-interval
    /// numbers (e.g. per render) should diff two snapshots.
    pub fn write_stats(&self) -> VfsWriteStats {
        self.stats
    }

    /// Write `contents` at `path` unless byte-identical content is
    /// already present there (bd-q3bxnq2e change-detection: the WASM
    /// render flushes its full artifact set every render, and skipping
    /// the no-op writes avoids re-cloning unchanged theme CSS / fonts /
    /// JS per keystroke).
    ///
    /// Returns `true` if a write happened, `false` if it was skipped.
    /// Skipped writes are counted in `skipped_writes` / `bytes_skipped`.
    ///
    /// Note: unlike [`VirtualFileSystem::add_file`], this borrows the
    /// contents — the clone happens only when the write does.
    pub fn add_file_if_changed(&mut self, path: &Path, contents: &[u8]) -> bool {
        let normalized = self.normalize_path(path);
        if self.files.get(&normalized).is_some_and(|existing| {
            // memcmp; equal-content compare costs a read pass but no
            // allocation or insert churn.
            existing.as_slice() == contents
        }) {
            self.stats.skipped_writes += 1;
            self.stats.bytes_skipped += contents.len();
            return false;
        }
        self.add_file(path, contents.to_vec());
        true
    }

    /// Update an existing file (same as add_file, but semantically clearer).
    pub fn update_file(&mut self, path: &Path, contents: Vec<u8>) {
        self.add_file(path, contents);
    }

    /// Remove a file from the virtual filesystem.
    ///
    /// Returns true if the file existed and was removed.
    pub fn remove_file(&mut self, path: &Path) -> bool {
        let normalized = self.normalize_path(path);
        self.files.remove(&normalized).is_some()
    }

    /// Add a directory (and all parent directories).
    pub fn add_directory(&mut self, path: &Path) {
        let normalized = self.normalize_path(path);
        self.add_directory_and_parents(&normalized);
    }

    /// Remove a directory.
    ///
    /// If recursive is false, only removes empty directories.
    /// If recursive is true, removes the directory and all contents.
    pub fn remove_directory(&mut self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        let normalized = self.normalize_path(path);

        if !self.directories.contains(&normalized) {
            return Err(not_found_error(&normalized));
        }

        // Find all files and subdirectories under this path
        let files_under: Vec<PathBuf> = self
            .files
            .keys()
            .filter(|p| p.starts_with(&normalized) && *p != &normalized)
            .cloned()
            .collect();

        let dirs_under: Vec<PathBuf> = self
            .directories
            .iter()
            .filter(|p| p.starts_with(&normalized) && *p != &normalized)
            .cloned()
            .collect();

        if !recursive && (!files_under.is_empty() || !dirs_under.is_empty()) {
            return Err(RuntimeError::Io(io::Error::new(
                io::ErrorKind::DirectoryNotEmpty,
                "Directory is not empty",
            )));
        }

        // Remove all files and directories under this path
        for file in files_under {
            self.files.remove(&file);
        }
        for dir in dirs_under {
            self.directories.remove(&dir);
        }
        self.directories.remove(&normalized);

        Ok(())
    }

    /// List all files in the virtual filesystem.
    pub fn list_files(&self) -> Vec<PathBuf> {
        self.files.keys().cloned().collect()
    }

    /// List contents of a directory.
    pub fn list_directory(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        let normalized = self.normalize_path(path);

        if !self.directories.contains(&normalized) {
            return Err(not_found_error(&normalized));
        }

        let mut entries: HashSet<PathBuf> = HashSet::new();

        // Find direct children (files)
        for file_path in self.files.keys() {
            if let Some(parent) = file_path.parent()
                && parent == normalized
            {
                entries.insert(file_path.clone());
            }
        }

        // Find direct children (directories)
        for dir_path in &self.directories {
            if let Some(parent) = dir_path.parent()
                && parent == normalized
                && dir_path != &normalized
            {
                entries.insert(dir_path.clone());
            }
        }

        Ok(entries.into_iter().collect())
    }

    /// Clear all files from the virtual filesystem.
    pub fn clear(&mut self) {
        self.files.clear();
        self.directories.clear();
        // Re-add root
        self.directories.insert(PathBuf::from("/"));
        self.directories.insert(self.project_root.clone());
    }

    /// Clear user files from the virtual filesystem, preserving files under the given prefix.
    ///
    /// This is used to clear project files while preserving embedded resources
    /// (like Bootstrap SCSS files under `/__quarto_resources__/`).
    pub fn clear_preserving_prefix(&mut self, preserved_prefix: &str) {
        // Retain files that start with the preserved prefix
        self.files
            .retain(|path, _| path.to_string_lossy().starts_with(preserved_prefix));

        // Retain directories that start with the preserved prefix
        // Also always keep root "/" and project root
        let project_root = self.project_root.clone();
        self.directories.retain(|path| {
            path == Path::new("/")
                || path == &project_root
                || path.to_string_lossy().starts_with(preserved_prefix)
        });

        // Re-add root and project root in case they were removed
        self.directories.insert(PathBuf::from("/"));
        self.directories.insert(self.project_root.clone());
    }

    /// Check if a path exists (as file or directory).
    pub fn exists(&self, path: &Path) -> bool {
        let normalized = self.normalize_path(path);
        self.files.contains_key(&normalized) || self.directories.contains(&normalized)
    }

    /// Check if a path is a file.
    pub fn is_file(&self, path: &Path) -> bool {
        let normalized = self.normalize_path(path);
        self.files.contains_key(&normalized)
    }

    /// Check if a path is a directory.
    pub fn is_directory(&self, path: &Path) -> bool {
        let normalized = self.normalize_path(path);
        self.directories.contains(&normalized)
    }

    /// Read file contents.
    pub fn read_file(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        let normalized = self.normalize_path(path);
        self.files
            .get(&normalized)
            .cloned()
            .ok_or_else(|| not_found_error(&normalized))
    }

    /// Get the size of a file.
    pub fn file_size(&self, path: &Path) -> Option<u64> {
        let normalized = self.normalize_path(path);
        self.files.get(&normalized).map(|c| c.len() as u64)
    }

    /// Get the project root directory.
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Normalize a path to an absolute path.
    pub fn normalize_path(&self, path: &Path) -> PathBuf {
        // If already absolute, just normalize
        if path.is_absolute() {
            return self.normalize_components(path);
        }
        // Otherwise, make it relative to project root
        let absolute = self.project_root.join(path);
        self.normalize_components(&absolute)
    }

    /// Normalize path components (remove . and resolve ..)
    fn normalize_components(&self, path: &Path) -> PathBuf {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    if !normalized.pop() {
                        // Can't go above root
                        normalized.push("/");
                    }
                }
                Component::CurDir => {
                    // Skip . components
                }
                other => {
                    normalized.push(other);
                }
            }
        }
        // Ensure we always have at least root
        if normalized.as_os_str().is_empty() {
            normalized.push("/");
        }
        normalized
    }

    /// Add a directory and all its parent directories.
    fn add_directory_and_parents(&mut self, path: &Path) {
        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);
            self.directories.insert(current.clone());
        }
    }
}

impl Drop for VirtualFileSystem {
    fn drop(&mut self) {
        // Diagnostic gauge for the bd-q3bxnq2e write-path investigation.
        // (On wasm32 `var_os` always returns None, so this never prints
        // there — quantification happens through the native proxy.)
        if std::env::var_os("QUARTO_PERF_STATS").is_some_and(|v| v == "1") {
            eprintln!(
                "perf.vfs-write writes={} bytes_written={} skipped_writes={} bytes_skipped={}",
                self.stats.writes,
                self.stats.bytes_written,
                self.stats.skipped_writes,
                self.stats.bytes_skipped,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_add_and_read_file() {
        let mut vfs = VirtualFileSystem::new();
        let path = Path::new("/project/test.txt");
        vfs.add_file(path, b"hello world".to_vec());

        assert!(vfs.is_file(path));
        assert_eq!(vfs.read_file(path).unwrap(), b"hello world");
    }

    #[test]
    fn test_vfs_creates_parent_directories() {
        let mut vfs = VirtualFileSystem::new();
        let path = Path::new("/project/deep/nested/dir/file.txt");
        vfs.add_file(path, b"content".to_vec());

        assert!(vfs.is_directory(Path::new("/project/deep")));
        assert!(vfs.is_directory(Path::new("/project/deep/nested")));
        assert!(vfs.is_directory(Path::new("/project/deep/nested/dir")));
    }

    #[test]
    fn test_vfs_remove_file() {
        let mut vfs = VirtualFileSystem::new();
        let path = Path::new("/project/test.txt");
        vfs.add_file(path, b"hello".to_vec());

        assert!(vfs.is_file(path));
        assert!(vfs.remove_file(path));
        assert!(!vfs.is_file(path));
        assert!(!vfs.remove_file(path)); // Second remove returns false
    }

    #[test]
    fn test_vfs_list_directory() {
        let mut vfs = VirtualFileSystem::new();
        vfs.add_file(Path::new("/project/file1.txt"), b"1".to_vec());
        vfs.add_file(Path::new("/project/file2.txt"), b"2".to_vec());
        vfs.add_file(Path::new("/project/subdir/file3.txt"), b"3".to_vec());

        let entries = vfs.list_directory(Path::new("/project")).unwrap();
        assert_eq!(entries.len(), 3); // file1, file2, subdir
    }

    #[test]
    fn test_vfs_relative_paths() {
        let mut vfs = VirtualFileSystem::new();
        // Add with relative path
        vfs.add_file(Path::new("test.txt"), b"hello".to_vec());

        // Should be accessible via both relative and absolute
        assert!(vfs.is_file(Path::new("test.txt")));
        assert!(vfs.is_file(Path::new("/project/test.txt")));
    }

    #[test]
    fn test_vfs_clear() {
        let mut vfs = VirtualFileSystem::new();
        vfs.add_file(Path::new("/project/test.txt"), b"hello".to_vec());

        assert!(vfs.is_file(Path::new("/project/test.txt")));
        vfs.clear();
        assert!(!vfs.is_file(Path::new("/project/test.txt")));
        // Root directories should still exist
        assert!(vfs.is_directory(Path::new("/")));
        assert!(vfs.is_directory(Path::new("/project")));
    }

    // === bd-q3bxnq2e: add_file_if_changed semantics ===

    #[test]
    fn test_add_file_if_changed_writes_new_file() {
        let mut vfs = VirtualFileSystem::new();
        let p = Path::new("/project/a.css");
        assert!(vfs.add_file_if_changed(p, b"body{}"));
        assert_eq!(vfs.read_file(p).unwrap(), b"body{}");
        let s = vfs.write_stats();
        assert_eq!(s.writes, 1);
        assert_eq!(s.bytes_written, 6);
        assert_eq!(s.skipped_writes, 0);
        assert_eq!(s.bytes_skipped, 0);
    }

    #[test]
    fn test_add_file_if_changed_overwrites_changed_content() {
        let mut vfs = VirtualFileSystem::new();
        let p = Path::new("/project/a.css");
        assert!(vfs.add_file_if_changed(p, b"v1"));
        assert!(vfs.add_file_if_changed(p, b"v2-longer"));
        assert_eq!(vfs.read_file(p).unwrap(), b"v2-longer");
        let s = vfs.write_stats();
        assert_eq!(s.writes, 2);
        assert_eq!(s.bytes_written, 2 + 9);
        assert_eq!(s.skipped_writes, 0);
    }

    #[test]
    fn test_add_file_if_changed_skips_identical_content() {
        let mut vfs = VirtualFileSystem::new();
        let p = Path::new("/project/a.css");
        assert!(vfs.add_file_if_changed(p, b"same-bytes"));
        // Identical second write must be skipped (return false), leave
        // the content readable, and count toward the skip stats.
        assert!(!vfs.add_file_if_changed(p, b"same-bytes"));
        assert_eq!(vfs.read_file(p).unwrap(), b"same-bytes");
        let s = vfs.write_stats();
        assert_eq!(s.writes, 1);
        assert_eq!(s.bytes_written, 10);
        assert_eq!(s.skipped_writes, 1);
        assert_eq!(s.bytes_skipped, 10);
    }

    #[test]
    fn test_add_file_if_changed_normalizes_paths() {
        // A relative spelling and its absolute normalization name the
        // same file — the second identical write must be skipped.
        let mut vfs = VirtualFileSystem::new();
        assert!(vfs.add_file_if_changed(Path::new("a.css"), b"x"));
        assert!(!vfs.add_file_if_changed(Path::new("/project/a.css"), b"x"));
        assert_eq!(vfs.write_stats().skipped_writes, 1);
    }

    #[test]
    fn test_add_file_if_changed_empty_content_still_writes() {
        // VFS-level semantics unchanged: empty bytes are a valid file.
        // The bd-3gtn empty-content skip lives at the flush sites (it
        // means "manifest entry, never write"), not in the VFS.
        let mut vfs = VirtualFileSystem::new();
        assert!(vfs.add_file_if_changed(Path::new("/project/empty"), b""));
        assert!(vfs.is_file(Path::new("/project/empty")));
        assert!(!vfs.add_file_if_changed(Path::new("/project/empty"), b""));
    }

    #[test]
    fn test_vfs_clear_preserving_prefix() {
        let mut vfs = VirtualFileSystem::new();

        // Add embedded resource files (should be preserved)
        vfs.add_file(
            Path::new("/__quarto_resources__/bootstrap/scss/_variables.scss"),
            b"$primary: blue;".to_vec(),
        );
        vfs.add_file(
            Path::new("/__quarto_resources__/bootstrap/scss/_mixins.scss"),
            b"@mixin foo {}".to_vec(),
        );

        // Add project files (should be cleared)
        vfs.add_file(Path::new("/project/index.qmd"), b"# Hello".to_vec());
        vfs.add_file(Path::new("/project/styles.scss"), b"body {}".to_vec());

        // Verify all files exist before clear
        assert!(vfs.is_file(Path::new(
            "/__quarto_resources__/bootstrap/scss/_variables.scss"
        )));
        assert!(vfs.is_file(Path::new(
            "/__quarto_resources__/bootstrap/scss/_mixins.scss"
        )));
        assert!(vfs.is_file(Path::new("/project/index.qmd")));
        assert!(vfs.is_file(Path::new("/project/styles.scss")));

        // Clear user files, preserving embedded resources
        vfs.clear_preserving_prefix("/__quarto_resources__");

        // Embedded resources should still exist
        assert!(vfs.is_file(Path::new(
            "/__quarto_resources__/bootstrap/scss/_variables.scss"
        )));
        assert!(vfs.is_file(Path::new(
            "/__quarto_resources__/bootstrap/scss/_mixins.scss"
        )));

        // Project files should be cleared
        assert!(!vfs.is_file(Path::new("/project/index.qmd")));
        assert!(!vfs.is_file(Path::new("/project/styles.scss")));

        // Root directories should still exist
        assert!(vfs.is_directory(Path::new("/")));
        assert!(vfs.is_directory(Path::new("/project")));

        // Embedded resource directories should still exist
        assert!(vfs.is_directory(Path::new("/__quarto_resources__")));
        assert!(vfs.is_directory(Path::new("/__quarto_resources__/bootstrap")));
        assert!(vfs.is_directory(Path::new("/__quarto_resources__/bootstrap/scss")));
    }
}
