/*
 * lua/runtime/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Runtime abstraction layer for Lua filters.
 *
 * This module provides a trait-based abstraction for system operations,
 * allowing Lua filters to run in different execution environments:
 *
 * - NativeRuntime: Full system access using std (default for native targets)
 * - WasmRuntime: Browser environment with VFS and fetch() (WASM targets)
 * - SandboxedRuntime: Restricted access for untrusted filters (decorator pattern)
 *
 * Design is informed by:
 * - [Deno's permission model](https://docs.deno.com/runtime/fundamentals/security/)
 * - [Node.js Permission Model](https://nodejs.org/api/permissions.html)
 *
 * Design doc: claude-notes/plans/2025-12-03-lua-runtime-abstraction-layer.md
 *
 * Related issues:
 * - k-475: Design and implement LuaRuntime abstraction layer
 * - k-482: LuaRuntime trait definition and module structure
 * - k-483: Implement NativeRuntime for Lua filters
 * - k-484: Implement WasmRuntime for browser execution
 * - k-485: Implement SandboxedRuntime for untrusted filters
 */

mod native;
mod sandbox;
mod traits;

// WASM runtime is only compiled for WASM targets
#[cfg(target_arch = "wasm32")]
mod wasm;

// Re-export core types (API surface for filter integration)
#[allow(unused_imports)]
pub use traits::{
    CommandOutput, LuaRuntime, PathKind, PathMetadata, RuntimeError, RuntimeResult, TempDir,
    XdgDirKind,
};

// Re-export runtime implementations
pub use native::NativeRuntime;

#[cfg(target_arch = "wasm32")]
pub use wasm::WasmRuntime;

// Re-export sandboxing types (API surface for k-485)
#[allow(unused_imports)]
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
