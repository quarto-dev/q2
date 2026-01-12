/*
 * wasm.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * WasmRuntime implementation for browser environments.
 *
 * This runtime operates within browser sandbox constraints:
 * - No direct filesystem access (uses VirtualFileSystem)
 * - No process execution
 * - Network via fetch() API
 * - No environment variables
 */

// This module is only compiled for WASM targets
#![cfg(target_arch = "wasm32")]

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::traits::{
    CommandOutput, PathKind, PathMetadata, RuntimeError, RuntimeResult, SystemRuntime, TempDir,
    XdgDirKind,
};

// =============================================================================
// JavaScript Interop for Template Rendering
// =============================================================================
//
// These extern declarations define the JavaScript functions that the host
// application (hub-client) must provide for template rendering.
//
// The functions are expected to be provided via a module at the path specified.
// In hub-client, this is typically at: /src/wasm-js-bridge/template.js
//
// JavaScript implementation requirements:
// - js_render_simple_template(template, dataJson): Promise<string>
//   Render a simple ${key} template with the provided JSON data
// - js_render_ejs(template, dataJson): Promise<string>
//   Render an EJS template with the provided JSON data
//
// Note: Data is passed as JSON strings to avoid complex type marshalling.
// The JavaScript side should JSON.parse the data before use.

#[wasm_bindgen(raw_module = "/src/wasm-js-bridge/template.js")]
extern "C" {
    /// Render a simple template with ${key} placeholders.
    ///
    /// # Arguments
    /// * `template` - The template string with ${key} placeholders
    /// * `data_json` - JSON-encoded object with key-value pairs for substitution
    ///
    /// # Returns
    /// A Promise that resolves to the rendered string, or rejects with an error.
    #[wasm_bindgen(js_name = "jsRenderSimpleTemplate", catch)]
    fn js_render_simple_template_impl(template: &str, data_json: &str) -> Result<JsValue, JsValue>;

    /// Render an EJS template with the given data.
    ///
    /// # Arguments
    /// * `template` - The EJS template string
    /// * `data_json` - JSON-encoded object with data for the template
    ///
    /// # Returns
    /// A Promise that resolves to the rendered string, or rejects with an error.
    #[wasm_bindgen(js_name = "jsRenderEjs", catch)]
    fn js_render_ejs_impl(template: &str, data_json: &str) -> Result<JsValue, JsValue>;

    /// Check if JavaScript template rendering is available.
    ///
    /// This can be used to gracefully degrade if the JS bridge is not set up.
    #[wasm_bindgen(js_name = "jsTemplateAvailable")]
    fn js_template_available_impl() -> bool;
}

/// Counter for generating unique temp directory names in WASM.
/// SystemTime::now() is not available in WASM, so we use a simple counter.
static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Helper function to create a "not found" error.
fn not_found_error(path: &Path) -> RuntimeError {
    RuntimeError::Io(io::Error::new(
        io::ErrorKind::NotFound,
        format!("Path not found: {}", path.display()),
    ))
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
}

impl VirtualFileSystem {
    /// Create a new empty virtual filesystem.
    pub fn new() -> Self {
        let mut vfs = Self {
            files: HashMap::new(),
            directories: HashSet::new(),
            project_root: PathBuf::from("/project"),
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
        self.files.insert(normalized, contents);
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
            if let Some(parent) = file_path.parent() {
                if parent == normalized {
                    entries.insert(file_path.clone());
                }
            }
        }

        // Find direct children (directories)
        for dir_path in &self.directories {
            if let Some(parent) = dir_path.parent() {
                if parent == normalized && dir_path != &normalized {
                    entries.insert(dir_path.clone());
                }
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

/// Runtime for WASM/browser environments.
///
/// This runtime operates within browser sandbox constraints:
/// - No direct filesystem access (uses VirtualFileSystem)
/// - No process execution
/// - Network via fetch() API
/// - No environment variables
pub struct WasmRuntime {
    /// Virtual filesystem for file operations.
    /// Uses RwLock to satisfy Send + Sync trait bounds.
    vfs: RwLock<VirtualFileSystem>,
}

impl WasmRuntime {
    /// Create a new WasmRuntime with an empty virtual filesystem.
    pub fn new() -> Self {
        Self {
            vfs: RwLock::new(VirtualFileSystem::new()),
        }
    }

    /// Create a WasmRuntime with a pre-populated virtual filesystem.
    pub fn with_vfs(vfs: VirtualFileSystem) -> Self {
        Self {
            vfs: RwLock::new(vfs),
        }
    }

    /// Add a file to the virtual filesystem.
    ///
    /// Convenience method that locks the VFS.
    pub fn add_file(&self, path: &Path, contents: Vec<u8>) {
        self.vfs.write().unwrap().add_file(path, contents);
    }

    /// Update a file in the virtual filesystem.
    pub fn update_file(&self, path: &Path, contents: Vec<u8>) {
        self.vfs.write().unwrap().update_file(path, contents);
    }

    /// Remove a file from the virtual filesystem.
    pub fn remove_file(&self, path: &Path) -> bool {
        self.vfs.write().unwrap().remove_file(path)
    }

    /// List all files in the virtual filesystem.
    pub fn list_files(&self) -> Vec<PathBuf> {
        self.vfs.read().unwrap().list_files()
    }

    /// Clear all files from the virtual filesystem.
    pub fn clear_files(&self) {
        self.vfs.write().unwrap().clear();
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

// Note: Using ?Send because WASM is single-threaded and JsFuture is not Send
#[async_trait(?Send)]
impl SystemRuntime for WasmRuntime {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.vfs.read().unwrap().read_file(path)
    }

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.vfs.write().unwrap().add_file(path, contents.to_vec());
        Ok(())
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        let vfs = self.vfs.read().unwrap();
        let exists = match kind {
            None => vfs.exists(path),
            Some(PathKind::File) => vfs.is_file(path),
            Some(PathKind::Directory) => vfs.is_directory(path),
            Some(PathKind::Symlink) => false, // VFS doesn't support symlinks
        };
        Ok(exists)
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        // In WASM, we normalize the path (no symlink resolution needed)
        Ok(self.vfs.read().unwrap().normalize_path(path))
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        let vfs = self.vfs.read().unwrap();

        if vfs.is_file(path) {
            let size = vfs.file_size(path).unwrap_or(0);
            Ok(PathMetadata {
                kind: PathKind::File,
                size,
                modified: None, // VFS doesn't track modification times
                accessed: None,
                readonly: false,
            })
        } else if vfs.is_directory(path) {
            Ok(PathMetadata {
                kind: PathKind::Directory,
                size: 0,
                modified: None,
                accessed: None,
                readonly: false,
            })
        } else {
            Err(not_found_error(path))
        }
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        let contents = self.file_read(src)?;
        self.file_write(dst, &contents)
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        let contents = self.file_read(old)?;
        self.file_write(new, &contents)?;
        self.vfs.write().unwrap().remove_file(old);
        Ok(())
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        if self.vfs.write().unwrap().remove_file(path) {
            Ok(())
        } else {
            Err(not_found_error(path))
        }
    }

    fn dir_create(&self, path: &Path, _recursive: bool) -> RuntimeResult<()> {
        // VFS always creates parent directories automatically
        self.vfs.write().unwrap().add_directory(path);
        Ok(())
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.vfs.write().unwrap().remove_directory(path, recursive)
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        self.vfs.read().unwrap().list_directory(path)
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        // Return the project root as CWD
        Ok(self.vfs.read().unwrap().project_root().to_path_buf())
    }

    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir> {
        // Create a temp directory in /tmp within VFS
        // Use a counter instead of SystemTime::now() since time is not available in WASM
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_name = format!("/tmp/{}-{}", template, counter);
        let temp_path = PathBuf::from(&temp_name);
        self.vfs.write().unwrap().add_directory(&temp_path);
        // Return a TempDir (cleanup won't work in VFS, but that's fine)
        Ok(TempDir::new(temp_path))
    }

    fn exec_pipe(&self, _command: &str, _args: &[&str], _stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        // Process execution is fundamentally not available in WASM
        Err(RuntimeError::NotSupported(
            "Process execution is not available in browser environment".to_string(),
        ))
    }

    fn exec_command(
        &self,
        _command: &str,
        _args: &[&str],
        _stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput> {
        Err(RuntimeError::NotSupported(
            "Process execution is not available in browser environment".to_string(),
        ))
    }

    fn env_get(&self, _name: &str) -> RuntimeResult<Option<String>> {
        // Environment variables don't exist in browser context
        Ok(None)
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        // Return empty map - no env vars in browser
        Ok(HashMap::new())
    }

    fn fetch_url(&self, _url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        // TODO: Implement using fetch() API via wasm-bindgen
        // For now, this is not supported in the core runtime
        Err(RuntimeError::NotSupported(
            "WasmRuntime fetch not yet implemented - use JavaScript fetch() directly".to_string(),
        ))
    }

    fn os_name(&self) -> &'static str {
        "wasm"
    }

    fn arch(&self) -> &'static str {
        "wasm32"
    }

    fn cpu_time(&self) -> RuntimeResult<u64> {
        Err(RuntimeError::NotSupported(
            "CPU time is not available in browser environment".to_string(),
        ))
    }

    fn xdg_dir(&self, _kind: XdgDirKind, _subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        Err(RuntimeError::NotSupported(
            "XDG directories are not available in browser environment".to_string(),
        ))
    }

    fn stdout_write(&self, _data: &[u8]) -> RuntimeResult<()> {
        // TODO: Could log to console.log via wasm-bindgen
        Ok(())
    }

    fn stderr_write(&self, _data: &[u8]) -> RuntimeResult<()> {
        // TODO: Could log to console.error via wasm-bindgen
        Ok(())
    }

    // =========================================================================
    // JavaScript Execution
    // =========================================================================
    //
    // These methods call out to browser JavaScript via wasm-bindgen.
    // The JavaScript implementation is provided by the host application (hub-client).
    //
    // This is asymmetric with NativeRuntime which embeds V8 - here we call OUT
    // to JavaScript rather than embedding a JS engine.

    fn js_available(&self) -> bool {
        // Check if the JS bridge is available
        // This calls the JavaScript function to verify the template module is loaded
        js_template_available_impl()
    }

    async fn js_render_simple_template(
        &self,
        template: &str,
        data: &serde_json::Value,
    ) -> RuntimeResult<String> {
        // Serialize data to JSON string
        let data_json = serde_json::to_string(data)
            .map_err(|e| RuntimeError::NotSupported(format!("Failed to serialize data: {}", e)))?;

        // Call the JavaScript function which returns a Promise
        let promise = js_render_simple_template_impl(template, &data_json).map_err(|e| {
            RuntimeError::NotSupported(format!("Failed to call jsRenderSimpleTemplate: {:?}", e))
        })?;

        // Await the Promise
        let result = JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|e| {
                RuntimeError::NotSupported(format!("Simple template rendering failed: {:?}", e))
            })?;

        // Convert result to String
        result
            .as_string()
            .ok_or_else(|| RuntimeError::NotSupported("Result was not a string".to_string()))
    }

    async fn render_ejs(&self, template: &str, data: &serde_json::Value) -> RuntimeResult<String> {
        // Serialize data to JSON string
        let data_json = serde_json::to_string(data)
            .map_err(|e| RuntimeError::NotSupported(format!("Failed to serialize data: {}", e)))?;

        // Call the JavaScript function which returns a Promise
        let promise = js_render_ejs_impl(template, &data_json).map_err(|e| {
            RuntimeError::NotSupported(format!("Failed to call jsRenderEjs: {:?}", e))
        })?;

        // Await the Promise
        let result = JsFuture::from(js_sys::Promise::from(promise))
            .await
            .map_err(|e| RuntimeError::NotSupported(format!("EJS rendering failed: {:?}", e)))?;

        // Convert result to String
        result
            .as_string()
            .ok_or_else(|| RuntimeError::NotSupported("Result was not a string".to_string()))
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

    #[test]
    fn test_wasm_runtime_file_operations() {
        let runtime = WasmRuntime::new();

        // Write a file
        runtime
            .file_write(Path::new("/project/test.qmd"), b"# Hello\n\nWorld")
            .unwrap();

        // Read it back
        let content = runtime.file_read(Path::new("/project/test.qmd")).unwrap();
        assert_eq!(content, b"# Hello\n\nWorld");

        // Check existence
        assert!(
            runtime
                .path_exists(Path::new("/project/test.qmd"), Some(PathKind::File))
                .unwrap()
        );
        assert!(
            runtime
                .path_exists(Path::new("/project"), Some(PathKind::Directory))
                .unwrap()
        );
    }

    #[test]
    fn test_wasm_runtime_cwd() {
        let runtime = WasmRuntime::new();
        let cwd = runtime.cwd().unwrap();
        assert_eq!(cwd, PathBuf::from("/project"));
    }

    #[test]
    fn test_wasm_runtime_dir_operations() {
        let runtime = WasmRuntime::new();

        // Create a directory
        runtime
            .dir_create(Path::new("/project/subdir"), false)
            .unwrap();
        assert!(
            runtime
                .path_exists(Path::new("/project/subdir"), Some(PathKind::Directory))
                .unwrap()
        );

        // Add a file in the directory
        runtime
            .file_write(Path::new("/project/subdir/file.txt"), b"content")
            .unwrap();

        // List the directory
        let entries = runtime.dir_list(Path::new("/project/subdir")).unwrap();
        assert_eq!(entries.len(), 1);

        // Can't remove non-empty directory without recursive
        assert!(
            runtime
                .dir_remove(Path::new("/project/subdir"), false)
                .is_err()
        );

        // Can remove with recursive
        runtime
            .dir_remove(Path::new("/project/subdir"), true)
            .unwrap();
        assert!(
            !runtime
                .path_exists(Path::new("/project/subdir"), None)
                .unwrap()
        );
    }

    #[test]
    fn test_wasm_runtime_file_copy() {
        let runtime = WasmRuntime::new();

        runtime
            .file_write(Path::new("/project/src.txt"), b"source content")
            .unwrap();
        runtime
            .file_copy(Path::new("/project/src.txt"), Path::new("/project/dst.txt"))
            .unwrap();

        let content = runtime.file_read(Path::new("/project/dst.txt")).unwrap();
        assert_eq!(content, b"source content");
    }

    #[test]
    fn test_wasm_runtime_path_rename() {
        let runtime = WasmRuntime::new();

        runtime
            .file_write(Path::new("/project/old.txt"), b"content")
            .unwrap();
        runtime
            .path_rename(Path::new("/project/old.txt"), Path::new("/project/new.txt"))
            .unwrap();

        assert!(
            !runtime
                .path_exists(Path::new("/project/old.txt"), None)
                .unwrap()
        );
        assert!(
            runtime
                .path_exists(Path::new("/project/new.txt"), None)
                .unwrap()
        );
    }

    #[test]
    fn test_wasm_runtime_process_not_supported() {
        let runtime = WasmRuntime::new();

        let result = runtime.exec_pipe("echo", &["hello"], &[]);
        assert!(matches!(result, Err(RuntimeError::NotSupported(_))));
    }
}
