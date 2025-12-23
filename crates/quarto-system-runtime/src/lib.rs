/*
 * quarto-system-runtime
 * Copyright (c) 2025 Posit, PBC
 *
 * Runtime abstraction layer for Quarto system operations.
 *
 * This crate provides a trait-based abstraction for system operations,
 * allowing Quarto tools to run in different execution environments:
 *
 * - NativeRuntime: Full system access using std (default for native targets)
 * - WasmRuntime: Browser environment with VFS and fetch() (WASM targets)
 * - SandboxedRuntime: Restricted access for untrusted code (decorator pattern)
 *
 * Design is informed by:
 * - [Deno's permission model](https://docs.deno.com/runtime/fundamentals/security/)
 * - [Node.js Permission Model](https://nodejs.org/api/permissions.html)
 */

mod native;
mod sandbox;
mod traits;

// WASM runtime is only compiled for WASM targets
#[cfg(target_arch = "wasm32")]
mod wasm;

// Re-export core types (API surface)
pub use traits::{
    CommandOutput, PathKind, PathMetadata, RuntimeError, RuntimeResult, SystemRuntime, TempDir,
    XdgDirKind,
};

// Re-export runtime implementations
pub use native::NativeRuntime;

#[cfg(target_arch = "wasm32")]
pub use wasm::WasmRuntime;

// Re-export sandboxing types
pub use sandbox::{PathPattern, SandboxedRuntime, SecurityPolicy, SharedRuntime};

/// Create a default runtime for the current platform.
///
/// On native targets, this returns a NativeRuntime with full system access.
/// On WASM targets, this returns a WasmRuntime with browser sandbox constraints.
#[cfg(not(target_arch = "wasm32"))]
pub fn default_runtime() -> NativeRuntime {
    NativeRuntime::new()
}

#[cfg(target_arch = "wasm32")]
pub fn default_runtime() -> WasmRuntime {
    WasmRuntime::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_runtime_exists() {
        let rt = default_runtime();
        // Basic sanity check
        assert!(!rt.os_name().is_empty());
        assert!(!rt.arch().is_empty());
    }

    #[test]
    fn test_native_runtime_file_operations() {
        let rt = NativeRuntime::new();
        let temp = rt.temp_dir("test").unwrap();

        let file_path = temp.path().join("test.txt");
        rt.file_write(&file_path, b"hello").unwrap();

        assert!(rt.path_exists(&file_path, None).unwrap());
        assert_eq!(rt.file_read(&file_path).unwrap(), b"hello");
    }

    #[test]
    fn test_sandboxed_runtime_passthrough() {
        // With trusted policy, should behave like inner runtime
        let inner = NativeRuntime::new();
        let policy = SecurityPolicy::trusted();
        let rt = SandboxedRuntime::new(inner, policy);

        // Should work like native runtime
        assert!(!rt.os_name().is_empty());
        let cwd = rt.cwd().unwrap();
        assert!(cwd.is_absolute());
    }
}
