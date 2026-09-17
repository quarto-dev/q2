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

pub mod cache_lru;
pub mod cache_versioning;
mod sandbox;
mod traits;

// In-memory virtual filesystem. Compiled on every target: WASM uses it
// as WasmRuntime's backing store; native builds use it in tests and in
// perf-harness drivers that exercise the WASM flush path (bd-q3bxnq2e).
mod vfs;

// Native runtime is only compiled for non-WASM targets
#[cfg(not(target_arch = "wasm32"))]
mod native;

// SASS compilation for native targets
#[cfg(not(target_arch = "wasm32"))]
pub mod sass_native;
#[cfg(not(target_arch = "wasm32"))]
pub use sass_native::{
    EmbeddedResourceProvider, RuntimeFs, compile_scss, compile_scss_with_embedded,
};

// WASM runtime is only compiled for WASM targets
#[cfg(target_arch = "wasm32")]
mod wasm;

// Re-export core types (API surface)
pub use cache_lru::{CACHE_LRU_INDEX_KEY, SASS_CACHE_BUDGET_BYTES, cache_get_lru, cache_set_lru};
pub use cache_versioning::{CACHE_VERSION_KEY, ensure_namespace_version};
pub use traits::{
    CacheEntryInfo, CommandOutput, PathKind, PathMetadata, RuntimeError, RuntimeResult, SassOutput,
    SystemRuntime, TempDir, XdgDirKind, validate_cache_key, validate_cache_namespace,
};
pub use vfs::{VfsWriteStats, VirtualFileSystem};

// Re-export runtime implementations based on target
#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
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

    /// Guard: the V8/deno_core dependency must not reappear in the
    /// workspace graph.
    ///
    /// This crate once embedded deno_core (→ rusty_v8) for EJS template
    /// rendering. The JS template path was removed in bd-3e3sam51 after
    /// bd-kuxzj8su migrated project scaffolding to quarto-doctemplate:
    /// v8 prebuilt archives are ~100MB build-time downloads, and rusty_v8
    /// publishes no musl prebuilts, which blocked static-musl release
    /// targets (PR #280). Anyone reintroducing a JS engine dependency
    /// should do so deliberately — behind a cargo feature, with the
    /// release-target implications worked through — not by accident.
    #[test]
    fn test_no_v8_in_workspace_lockfile() {
        let lockfile = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock");
        let contents = std::fs::read_to_string(&lockfile)
            .unwrap_or_else(|e| panic!("read {}: {e}", lockfile.display()));

        for banned in ["v8", "deno_core", "serde_v8"] {
            let needle = format!("name = \"{banned}\"");
            assert!(
                !contents.contains(&needle),
                "workspace Cargo.lock contains package `{banned}` — \
                 the V8/deno_core dependency was removed in bd-3e3sam51 \
                 and must not be reintroduced accidentally"
            );
        }
    }
}
