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

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::traits::{
    CommandOutput, PathKind, PathMetadata, RuntimeError, RuntimeResult, SystemRuntime, TempDir,
    XdgDirKind,
};

/// Runtime for WASM/browser environments.
///
/// This runtime operates within browser sandbox constraints:
/// - No direct filesystem access (uses VirtualFileSystem)
/// - No process execution
/// - Network via fetch() API
/// - No environment variables
pub struct WasmRuntime {
    // TODO: Add VirtualFileSystem
    // vfs: VirtualFileSystem,
}

impl WasmRuntime {
    /// Create a new WasmRuntime.
    ///
    /// In the future, this will accept a MediaBag to pre-populate the VFS.
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for WasmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemRuntime for WasmRuntime {
    fn file_read(&self, _path: &Path) -> RuntimeResult<Vec<u8>> {
        // TODO: Implement using VirtualFileSystem
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn file_write(&self, _path: &Path, _contents: &[u8]) -> RuntimeResult<()> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn path_exists(&self, _path: &Path, _kind: Option<PathKind>) -> RuntimeResult<bool> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        // In WASM, we can't resolve symlinks or verify path existence.
        // We just normalize the path by removing . and .. components.
        use std::path::Component;

        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::ParentDir => {
                    // Go up one level if possible
                    if !normalized.pop() {
                        // Can't go above root - keep the ..
                        normalized.push("..");
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
        Ok(normalized)
    }

    fn path_metadata(&self, _path: &Path) -> RuntimeResult<PathMetadata> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn file_copy(&self, _src: &Path, _dst: &Path) -> RuntimeResult<()> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn path_rename(&self, _old: &Path, _new: &Path) -> RuntimeResult<()> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn file_remove(&self, _path: &Path) -> RuntimeResult<()> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime file operations not yet implemented".to_string(),
        ))
    }

    fn dir_create(&self, _path: &Path, _recursive: bool) -> RuntimeResult<()> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime directory operations not yet implemented".to_string(),
        ))
    }

    fn dir_remove(&self, _path: &Path, _recursive: bool) -> RuntimeResult<()> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime directory operations not yet implemented".to_string(),
        ))
    }

    fn dir_list(&self, _path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime directory operations not yet implemented".to_string(),
        ))
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        // CWD is inherently not supported in browser environments
        Err(RuntimeError::NotSupported(
            "Current working directory is not available in browser environment".to_string(),
        ))
    }

    fn temp_dir(&self, _template: &str) -> RuntimeResult<TempDir> {
        Err(RuntimeError::NotSupported(
            "WasmRuntime temp directory not yet implemented".to_string(),
        ))
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
        Err(RuntimeError::NotSupported(
            "WasmRuntime fetch not yet implemented".to_string(),
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
}
