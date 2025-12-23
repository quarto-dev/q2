/*
 * lua/runtime/mod.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Runtime abstraction layer for Lua filters.
 *
 * This module re-exports the runtime abstraction from quarto-system-runtime,
 * allowing Lua filters to run in different execution environments:
 *
 * - NativeRuntime: Full system access using std (default for native targets)
 * - WasmRuntime: Browser environment with VFS and fetch() (WASM targets)
 * - SandboxedRuntime: Restricted access for untrusted filters (decorator pattern)
 *
 * Design is informed by:
 * - [Deno's permission model](https://docs.deno.com/runtime/fundamentals/security/)
 * - [Node.js Permission Model](https://nodejs.org/api/permissions.html)
 */

// Re-export everything from quarto-system-runtime
pub use quarto_system_runtime::*;
