# Lua Runtime Abstraction Layer Design

**Date**: 2025-12-03 (Updated: 2025-12-03)
**Related Issues**: k-475, k-473
**Status**: Design Document

---

## Executive Summary

This document proposes a design for a Lua runtime abstraction layer that enables:

1. **WASM targets**: Executing Lua filters in browser environments
2. **Low-permission runtimes**: Sandboxed execution for untrusted filters
3. **Print/logging capture**: Redirecting output for diagnostic purposes
4. **Network abstraction**: Using `fetch()` in WASM instead of sockets

The design uses a **dependency injection pattern** with a trait-based abstraction layer that sits between Lua filter code and underlying system operations.

### Industry Alignment

This design is informed by established permission systems in modern runtimes:

- **[Deno](https://docs.deno.com/runtime/fundamentals/security/)**: Secure by default with granular `--allow-*` and `--deny-*` flags
- **[Node.js Permission Model](https://nodejs.org/api/permissions.html)**: Experimental (stable in v23.5+) with `--allow-fs-*` patterns

Key patterns adopted from these systems:
- **Secure by default**: No permissions granted without explicit opt-in
- **Granular scoping**: Permissions can target specific paths, hosts, programs
- **Deny takes precedence**: Explicit denials override allows
- **Detailed error messages**: Clear explanations for permission denials

---

## Industry Research: Runtime Permission Systems

This section documents the permission systems in modern JavaScript runtimes, which inform our design.

### Deno Permission System

**Reference**: [Deno Security Documentation](https://docs.deno.com/runtime/fundamentals/security/)

Deno pioneered the "secure by default" runtime model. Key characteristics:

| Flag | Scope | Examples |
|------|-------|----------|
| `--allow-read` | File paths | `--allow-read=/tmp`, `--allow-read=./src` |
| `--allow-write` | File paths | `--allow-write=/tmp/output` |
| `--allow-net` | Hosts/ports | `--allow-net=example.com:443` |
| `--allow-env` | Variable names | `--allow-env=HOME,PATH`, `--allow-env=AWS_*` |
| `--allow-run` | Program names | `--allow-run=git,python` |
| `--allow-sys` | System APIs | `--allow-sys=hostname,osRelease` |
| `--allow-ffi` | Library paths | `--allow-ffi=./mylib.so` |

**Key patterns**:
1. **Deny takes precedence**: `--deny-net=evil.com --allow-net` blocks evil.com but allows others
2. **Suffix wildcards**: `--allow-env=AWS_*` matches `AWS_ACCESS_KEY_ID`, etc.
3. **Runtime API**: `Deno.permissions.query({ name: "read", path: "/tmp" })`
4. **Audit logging**: `DENO_AUDIT_PERMISSIONS=/path/to/log.jsonl`
5. **Config file support** (Deno 2.5+): Permissions can be declared in `deno.json`

**CWD handling**: `Deno.chdir()` requires `--allow-read` permission on the target directory.

### Node.js Permission Model

**Reference**: [Node.js Permissions Documentation](https://nodejs.org/api/permissions.html)

Node.js added an experimental permission model in v20, stabilized in v23.5:

| Flag | Scope | Examples |
|------|-------|----------|
| `--allow-fs-read` | File paths | `--allow-fs-read=/tmp/*`, `--allow-fs-read=.` |
| `--allow-fs-write` | File paths | `--allow-fs-write=/tmp/output` |
| `--allow-child-process` | Boolean | `--allow-child-process` |
| `--allow-worker` | Boolean | `--allow-worker` |
| `--allow-net` | Boolean | `--allow-net` |
| `--allow-addons` | Boolean | `--allow-addons` |
| `--allow-wasi` | Boolean | `--allow-wasi` |

**Key patterns**:
1. **Wildcards**: `--allow-fs-read=/home/test*` matches `/home/test`, `/home/test2`, etc.
2. **Runtime API**: `process.permission.has('fs.read', '/path')`
3. **Seat belt model**: Protects against unintentional access, not malicious code

**Limitations noted**:
- Symlinks can bypass path restrictions
- Permissions don't inherit to worker threads
- Existing file descriptors bypass the model

### Bun

**Reference**: [Bun GitHub Issue #6617](https://github.com/oven-sh/bun/issues/6617)

Bun currently has **no permission model**. There is an open feature request for sandboxing support similar to Deno. The Bun team has discussed:
- Binary dead code elimination based on static analysis
- Possible future runtime permission checks

### Comparison Summary

| Feature | Deno | Node.js | Our Design |
|---------|------|---------|------------|
| Secure by default | ✅ | ✅ (with `--permission`) | ✅ |
| Path wildcards | ✅ | ✅ | ✅ |
| Deny lists | ✅ | ❌ | ✅ |
| Env var scoping | ✅ | ❌ | ✅ |
| Program scoping | ✅ | ❌ (boolean only) | ✅ |
| Runtime query API | ✅ | ✅ | Planned |
| Config file support | ✅ (Deno 2.5+) | ❌ | Future |
| Audit logging | ✅ | ❌ | Future |

**Design choice**: We adopt Deno's model as the more mature and feature-complete system, with Node.js compatibility considered where practical.

---

## Problem Statement

### Current State

The current implementation (`filter.rs:105`) creates Lua state via:

```rust
let lua = Lua::new();  // Loads ALL_SAFE libraries including IO and OS
```

This means filters currently have unrestricted access to:
- **`os.execute()`** - Execute arbitrary shell commands
- **`io.open()`** - Read/write any file on the filesystem
- **`os.getenv()`** - Read environment variables (potential secret leakage)
- **`io.popen()`** - Open process pipes
- **`require()`** - Load arbitrary Lua modules
- **`os.remove()`** / **`os.rename()`** - Delete/move files

### Required Capabilities

To support the Pandoc Lua API (k-473), we need to provide equivalents for **41 syscall-like operations** across:

| Category | Count | Examples |
|----------|-------|----------|
| Process Execution | 3 | `pipe`, `command`, `os.execute` |
| File I/O | 2 | `read_file`, `write_file` |
| Directory Ops | 8 | `make_directory`, `list_directory`, `with_working_directory` |
| Path Manipulation | 11 | `join`, `normalize`, `exists`, `split` |
| Environment | 3 | `environment`, `with_environment`, `os.getenv` |
| Media/Network | 8 | `fetch`, `insert`, `lookup` |
| System Info | 4 | `os`, `arch`, `cputime`, `xdg` |

---

## Design Goals

1. **Full Pandoc API compatibility** with restriction enforcement (not function removal)
2. **Clean separation** between Lua API layer and system operations
3. **Compile-time safety** through Rust's type system
4. **Minimal overhead** for native execution
5. **WASM-first design** that degrades gracefully

---

## Architecture

### Layered Design

```
┌─────────────────────────────────────────────────────────────────┐
│                     Lua Filter Code                              │
│  (pandoc.system.read_file, os.execute, io.open, etc.)           │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Pandoc Lua API Layer                           │
│  (register_pandoc_namespace, pandoc.system, pandoc.path, etc.)  │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │    Lua Standard Library Replacements                        ││
│  │    (io.open → runtime.open_file, os.execute → runtime.exec) ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│              LuaRuntime Trait (Abstraction Layer)               │
│                                                                  │
│  file_read()  file_write()  exec_command()  fetch_url()  ...    │
└─────────────────────────────────────────────────────────────────┘
                              │
            ┌─────────────────┼─────────────────┐
            ▼                 ▼                 ▼
┌───────────────────┐ ┌───────────────────┐ ┌───────────────────┐
│   NativeRuntime   │ │   WasmRuntime     │ │ SandboxedRuntime  │
│                   │ │                   │ │                   │
│ - std::fs         │ │ - Emscripten FS   │ │ - Path validation │
│ - std::process    │ │ - fetch() API     │ │ - Allowlist       │
│ - std::env        │ │ - No processes    │ │ - Wraps any other │
└───────────────────┘ └───────────────────┘ └───────────────────┘
```

### Core Trait Definition

```rust
/// Result type for runtime operations
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Errors that can occur during runtime operations
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Operation not supported: {0}")]
    NotSupported(String),

    #[error("Path outside allowed boundary: {0}")]
    PathViolation(PathBuf),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Process execution failed: {code} - {message}")]
    ProcessFailed { code: i32, message: String },
}

/// Trait defining all low-level runtime operations
///
/// Implementations of this trait provide the actual system interaction,
/// allowing for different behavior based on target (native, WASM) or
/// security policy (trusted, sandboxed).
pub trait LuaRuntime: Send + Sync {
    // ═══════════════════════════════════════════════════════════════
    // FILE OPERATIONS
    // ═══════════════════════════════════════════════════════════════

    /// Read entire file contents
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>>;

    /// Read file as string (with encoding detection)
    fn file_read_string(&self, path: &Path) -> RuntimeResult<String>;

    /// Write bytes to file (creates or overwrites)
    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()>;

    /// Check if path exists, optionally filtering by type
    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool>;

    /// Get file/directory metadata
    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata>;

    /// Copy file preserving permissions
    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()>;

    /// Rename/move file or directory
    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()>;

    /// Delete file
    fn file_remove(&self, path: &Path) -> RuntimeResult<()>;

    // ═══════════════════════════════════════════════════════════════
    // DIRECTORY OPERATIONS
    // ═══════════════════════════════════════════════════════════════

    /// Create directory (optionally with parents)
    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()>;

    /// Remove directory (optionally with contents)
    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()>;

    /// List directory entries
    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>>;

    /// Get current working directory
    fn cwd(&self) -> RuntimeResult<PathBuf>;

    /// Create temporary directory, returns path
    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir>;

    // ═══════════════════════════════════════════════════════════════
    // PROCESS EXECUTION
    // ═══════════════════════════════════════════════════════════════

    /// Execute command with stdin input, return stdout
    ///
    /// This is the `pandoc.pipe` equivalent.
    fn exec_pipe(
        &self,
        command: &str,
        args: &[&str],
        stdin: &[u8],
    ) -> RuntimeResult<Vec<u8>>;

    /// Execute command with full output capture
    ///
    /// This is the `pandoc.system.command` equivalent.
    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput>;

    // ═══════════════════════════════════════════════════════════════
    // ENVIRONMENT
    // ═══════════════════════════════════════════════════════════════

    /// Get single environment variable
    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>>;

    /// Get all environment variables
    fn env_all(&self) -> RuntimeResult<HashMap<String, String>>;

    // ═══════════════════════════════════════════════════════════════
    // NETWORK
    // ═══════════════════════════════════════════════════════════════

    /// Fetch content from URL
    ///
    /// Returns (content, mime_type)
    fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)>;

    // ═══════════════════════════════════════════════════════════════
    // SYSTEM INFO
    // ═══════════════════════════════════════════════════════════════

    /// Operating system identifier (darwin, linux, windows, wasm, etc.)
    fn os_name(&self) -> &'static str;

    /// Machine architecture (x86_64, aarch64, wasm32, etc.)
    fn arch(&self) -> &'static str;

    /// CPU time used (in picoseconds, if available)
    fn cpu_time(&self) -> RuntimeResult<u64>;

    /// XDG base directory lookup
    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf>;

    // ═══════════════════════════════════════════════════════════════
    // OUTPUT
    // ═══════════════════════════════════════════════════════════════

    /// Handle print() output
    fn print(&self, message: &str);

    /// Handle io.write() to stdout
    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()>;

    /// Handle io.write() to stderr
    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()>;
}

// ═══════════════════════════════════════════════════════════════════
// SUPPORTING TYPES
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone)]
pub struct PathMetadata {
    pub kind: PathKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub accessed: Option<SystemTime>,
    pub readonly: bool,
}

#[derive(Debug)]
pub struct CommandOutput {
    /// Exit code (0 = success)
    pub code: i32,
    /// Standard output
    pub stdout: Vec<u8>,
    /// Standard error
    pub stderr: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XdgDirKind {
    Config,
    Data,
    Cache,
    State,
    ConfigDirs,  // Returns list
    DataDirs,    // Returns list
}

/// RAII guard for temporary directory that cleans up on drop
pub struct TempDir {
    path: PathBuf,
    // Cleanup behavior depends on runtime
}
```

---

## Runtime Implementations

### NativeRuntime

The default implementation for native targets using `std`:

```rust
pub struct NativeRuntime {
    /// Optional HTTP client for network operations
    #[cfg(feature = "network")]
    http_client: reqwest::blocking::Client,
}

impl Default for NativeRuntime {
    fn default() -> Self {
        Self {
            #[cfg(feature = "network")]
            http_client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("Failed to create HTTP client"),
        }
    }
}

impl LuaRuntime for NativeRuntime {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        Ok(std::fs::read(path)?)
    }

    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        use std::process::{Command, Stdio};

        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin_pipe) = child.stdin.take() {
            std::io::Write::write_all(&mut stdin_pipe, stdin)?;
        }

        let output = child.wait_with_output()?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RuntimeError::ProcessFailed {
                code: output.status.code().unwrap_or(-1),
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        }
    }

    fn os_name(&self) -> &'static str {
        std::env::consts::OS
    }

    // ... other implementations use std equivalents
}
```

### WasmRuntime

Implementation for WASM/Emscripten targets. The WASM runtime has inherent limitations due to the browser sandbox:

| Operation | WASM Behavior |
|-----------|---------------|
| File I/O | Virtual filesystem only (mediabag + in-memory) |
| Process execution | Always `NotSupported` |
| Network | Via `fetch()` API (blocking wrapper) |
| Environment | Not available (returns `None` or empty) |
| CWD | Not supported |
| System info | Returns "wasm" / "wasm32" |

```rust
#[cfg(target_arch = "wasm32")]
pub struct WasmRuntime {
    /// Virtual filesystem (Emscripten's MEMFS or custom)
    vfs: VirtualFileSystem,
}

#[cfg(target_arch = "wasm32")]
impl LuaRuntime for WasmRuntime {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        // Use Emscripten's file system API or custom VFS
        self.vfs.read(path)
    }

    fn exec_pipe(&self, _command: &str, _args: &[&str], _stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        // Process execution is not available in WASM
        Err(RuntimeError::NotSupported(
            "Process execution is not available in browser environment".to_string()
        ))
    }

    fn exec_command(&self, _command: &str, _args: &[&str], _stdin: Option<&[u8]>) -> RuntimeResult<CommandOutput> {
        Err(RuntimeError::NotSupported(
            "Process execution is not available in browser environment".to_string()
        ))
    }

    fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        // Use JavaScript fetch() API via wasm-bindgen
        // NOTE: This blocks on the async fetch() - see "Async Network" decision
        wasm_fetch_blocking(url)
    }

    fn os_name(&self) -> &'static str {
        "wasm"  // Could also detect browser via JS if needed
    }

    fn arch(&self) -> &'static str {
        "wasm32"
    }

    // Environment is not available in browser
    fn env_get(&self, _name: &str) -> RuntimeResult<Option<String>> {
        Ok(None)
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        Ok(HashMap::new())
    }

    // CWD is not supported in WASM
    fn cwd(&self) -> RuntimeResult<PathBuf> {
        Err(RuntimeError::NotSupported(
            "Current working directory is not available in browser environment".to_string()
        ))
    }
}

// JavaScript interop for fetch (blocking wrapper)
#[cfg(target_arch = "wasm32")]
fn wasm_fetch_blocking(url: &str) -> RuntimeResult<(Vec<u8>, String)> {
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;

    // Use pollster or similar to block on the async fetch
    // This is a simplified placeholder - real implementation would use:
    // 1. web_sys::window().fetch_with_str(url)
    // 2. Convert JsFuture to blocking via pollster::block_on or similar
    // 3. Extract body bytes and content-type header
    todo!("Implement WASM fetch blocking wrapper")
}
```

### SandboxedRuntime

A decorator pattern that wraps any other runtime with security policies.

#### Design Principles (from Deno/Node.js)

Following established patterns from [Deno's security model](https://docs.deno.com/runtime/fundamentals/security/) and [Node.js Permission Model](https://nodejs.org/api/permissions.html):

1. **Secure by default**: Start with no permissions, explicitly grant what's needed
2. **Allow + Deny pattern**: Support both allowlists and denylists; deny takes precedence
3. **Granular scoping**:
   - File paths: Support specific files, directories, and wildcards
   - Network: Support host:port patterns
   - Processes: Support specific program names
   - Environment: Support specific variable names
4. **Clear categories** (aligned with Deno's flags):
   - `read` / `write` - File system access (like `--allow-read`, `--allow-write`)
   - `net` - Network access (like `--allow-net`)
   - `run` - Process execution (like `--allow-run`)
   - `env` - Environment access including CWD (like `--allow-env`)
   - `sys` - System info (like `--allow-sys`)

```rust
/// A path pattern that can match files/directories
/// Supports:
/// - Exact paths: "/home/user/file.txt"
/// - Directory prefixes: "/home/user/" (matches everything under)
/// - Wildcards: "/home/user/*.lua" (matches pattern)
#[derive(Debug, Clone)]
pub struct PathPattern(String);

impl PathPattern {
    pub fn new(pattern: impl Into<String>) -> Self {
        Self(pattern.into())
    }

    /// Match a path against this pattern
    pub fn matches(&self, path: &Path) -> bool {
        let pattern = &self.0;
        let path_str = path.to_string_lossy();

        if pattern.contains('*') {
            // Glob-style matching
            glob_match(pattern, &path_str)
        } else if pattern.ends_with('/') || pattern.ends_with(std::path::MAIN_SEPARATOR) {
            // Directory prefix
            path_str.starts_with(pattern.trim_end_matches(['/', '\\']))
        } else {
            // Exact match or prefix
            path_str == pattern.as_str() || path_str.starts_with(pattern)
        }
    }
}

/// Security policy for sandboxed execution
///
/// Modeled after Deno's permission flags:
/// - https://docs.deno.com/runtime/fundamentals/security/
///
/// And Node.js Permission Model:
/// - https://nodejs.org/api/permissions.html
#[derive(Debug, Clone, Default)]
pub struct SecurityPolicy {
    // ═══════════════════════════════════════════════════════════════
    // FILE SYSTEM (like Deno's --allow-read, --allow-write)
    // ═══════════════════════════════════════════════════════════════

    /// Paths allowed for reading. Empty = no read access.
    /// Use PathPattern::new("*") for unrestricted access.
    pub allow_read: Vec<PathPattern>,

    /// Paths explicitly denied for reading. Takes precedence over allow_read.
    pub deny_read: Vec<PathPattern>,

    /// Paths allowed for writing. Empty = no write access.
    pub allow_write: Vec<PathPattern>,

    /// Paths explicitly denied for writing. Takes precedence over allow_write.
    pub deny_write: Vec<PathPattern>,

    // ═══════════════════════════════════════════════════════════════
    // NETWORK (like Deno's --allow-net)
    // ═══════════════════════════════════════════════════════════════

    /// Allowed network hosts/URLs. Empty = no network access.
    /// Examples: "example.com", "example.com:443", "https://api.example.com"
    pub allow_net: Vec<String>,

    /// Denied network hosts. Takes precedence over allow_net.
    pub deny_net: Vec<String>,

    // ═══════════════════════════════════════════════════════════════
    // PROCESS EXECUTION (like Deno's --allow-run)
    // ═══════════════════════════════════════════════════════════════

    /// Allowed programs to execute. Empty = no process execution.
    /// Examples: "git", "python", "/usr/bin/pandoc"
    pub allow_run: Vec<String>,

    /// Denied programs. Takes precedence over allow_run.
    pub deny_run: Vec<String>,

    // ═══════════════════════════════════════════════════════════════
    // ENVIRONMENT (like Deno's --allow-env)
    // Controls: environment variables AND current working directory
    // ═══════════════════════════════════════════════════════════════

    /// Allowed environment variables. Empty = no env access.
    /// Use "*" for all variables. Supports suffix wildcards like "AWS_*".
    pub allow_env: Vec<String>,

    /// Denied environment variables. Takes precedence over allow_env.
    pub deny_env: Vec<String>,

    /// Whether CWD operations are allowed.
    /// Part of "env" category since CWD is process-global state.
    /// - `true`: Allow get_cwd(), chdir(), with_working_directory()
    /// - `false`: Deny CWD operations
    pub allow_cwd: bool,

    // ═══════════════════════════════════════════════════════════════
    // SYSTEM INFO (like Deno's --allow-sys)
    // ═══════════════════════════════════════════════════════════════

    /// Allowed system info APIs. Empty = no sys info.
    /// Options: "hostname", "osRelease", "osUptime", "loadavg",
    ///          "networkInterfaces", "systemMemoryInfo", "uid", "gid", "cpuinfo"
    pub allow_sys: Vec<String>,
}

impl SecurityPolicy {
    /// Fully permissive policy (for trusted filters)
    ///
    /// Equivalent to Deno's `-A` / `--allow-all` flag.
    pub fn trusted() -> Self {
        Self {
            allow_read: vec![PathPattern::new("*")],
            deny_read: vec![],
            allow_write: vec![PathPattern::new("*")],
            deny_write: vec![],
            allow_net: vec!["*".to_string()],
            deny_net: vec![],
            allow_run: vec!["*".to_string()],
            deny_run: vec![],
            allow_env: vec!["*".to_string()],
            deny_env: vec![],
            allow_cwd: true,
            allow_sys: vec!["*".to_string()],
        }
    }

    /// Restrictive policy for untrusted filters
    ///
    /// Only allows:
    /// - Reading from project directory
    /// - Writing to _output subdirectory
    /// - No network, process execution, or environment access
    pub fn untrusted(project_root: PathBuf) -> Self {
        let project_str = project_root.to_string_lossy().to_string();
        let output_str = project_root.join("_output").to_string_lossy().to_string();

        Self {
            allow_read: vec![PathPattern::new(format!("{}/", project_str))],
            deny_read: vec![],
            allow_write: vec![PathPattern::new(format!("{}/", output_str))],
            deny_write: vec![],
            allow_net: vec![],
            deny_net: vec![],
            allow_run: vec![],
            deny_run: vec![],
            allow_env: vec![],
            deny_env: vec![],
            allow_cwd: false,  // No CWD access in untrusted mode
            allow_sys: vec!["osRelease".to_string()],  // Only basic system info
        }
    }

    /// Check if a path can be read
    pub fn can_read(&self, path: &Path) -> Result<(), PermissionError> {
        // Deny takes precedence (Deno pattern)
        if self.deny_read.iter().any(|p| p.matches(path)) {
            return Err(PermissionError::Denied {
                operation: "read",
                resource: path.display().to_string(),
                reason: "path is in deny_read list".to_string(),
            });
        }

        if self.allow_read.iter().any(|p| p.matches(path)) {
            return Ok(());
        }

        Err(PermissionError::NotAllowed {
            operation: "read",
            resource: path.display().to_string(),
            hint: "add path to allow_read in SecurityPolicy".to_string(),
        })
    }

    /// Check if a path can be written
    pub fn can_write(&self, path: &Path) -> Result<(), PermissionError> {
        if self.deny_write.iter().any(|p| p.matches(path)) {
            return Err(PermissionError::Denied {
                operation: "write",
                resource: path.display().to_string(),
                reason: "path is in deny_write list".to_string(),
            });
        }

        if self.allow_write.iter().any(|p| p.matches(path)) {
            return Ok(());
        }

        Err(PermissionError::NotAllowed {
            operation: "write",
            resource: path.display().to_string(),
            hint: "add path to allow_write in SecurityPolicy".to_string(),
        })
    }

    /// Check if a program can be executed
    pub fn can_run(&self, program: &str) -> Result<(), PermissionError> {
        if self.deny_run.iter().any(|p| p == program || p == "*") {
            return Err(PermissionError::Denied {
                operation: "run",
                resource: program.to_string(),
                reason: "program is in deny_run list".to_string(),
            });
        }

        if self.allow_run.iter().any(|p| p == program || p == "*") {
            return Ok(());
        }

        Err(PermissionError::NotAllowed {
            operation: "run",
            resource: program.to_string(),
            hint: "add program name to allow_run in SecurityPolicy".to_string(),
        })
    }

    /// Check if an environment variable can be accessed
    pub fn can_env(&self, name: &str) -> Result<(), PermissionError> {
        if self.deny_env.iter().any(|p| env_pattern_matches(p, name)) {
            return Err(PermissionError::Denied {
                operation: "env",
                resource: name.to_string(),
                reason: "variable is in deny_env list".to_string(),
            });
        }

        if self.allow_env.iter().any(|p| env_pattern_matches(p, name)) {
            return Ok(());
        }

        Err(PermissionError::NotAllowed {
            operation: "env",
            resource: name.to_string(),
            hint: "add variable name to allow_env in SecurityPolicy".to_string(),
        })
    }

    /// Check if CWD operations are allowed
    pub fn can_cwd(&self) -> Result<(), PermissionError> {
        if self.allow_cwd {
            Ok(())
        } else {
            Err(PermissionError::NotAllowed {
                operation: "cwd",
                resource: "current working directory".to_string(),
                hint: "set allow_cwd = true in SecurityPolicy".to_string(),
            })
        }
    }

    /// Check if a network host can be accessed
    pub fn can_net(&self, host: &str) -> Result<(), PermissionError> {
        if self.deny_net.iter().any(|p| host_pattern_matches(p, host)) {
            return Err(PermissionError::Denied {
                operation: "net",
                resource: host.to_string(),
                reason: "host is in deny_net list".to_string(),
            });
        }

        if self.allow_net.iter().any(|p| host_pattern_matches(p, host)) {
            return Ok(());
        }

        Err(PermissionError::NotAllowed {
            operation: "net",
            resource: host.to_string(),
            hint: "add host to allow_net in SecurityPolicy".to_string(),
        })
    }
}

/// Detailed permission error with context for debugging
///
/// Design decision: Provide detailed error messages to help users understand
/// and fix permission issues. This follows Deno's approach of helpful errors.
#[derive(Debug, thiserror::Error)]
pub enum PermissionError {
    #[error("Permission denied: {operation} access to '{resource}' - {reason}")]
    Denied {
        operation: &'static str,
        resource: String,
        reason: String,
    },

    #[error("Permission not granted: {operation} access to '{resource}' - {hint}")]
    NotAllowed {
        operation: &'static str,
        resource: String,
        hint: String,
    },
}

/// Match environment variable pattern (supports suffix wildcards like "AWS_*")
fn env_pattern_matches(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        name.starts_with(prefix)
    } else {
        pattern == name
    }
}

/// Match host pattern (supports wildcards and port specifications)
fn host_pattern_matches(pattern: &str, host: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    // Simple prefix match for now; could be extended for more complex patterns
    host.starts_with(pattern) || pattern == host
}

/// Sandboxed runtime that enforces security policies
pub struct SandboxedRuntime<R: LuaRuntime> {
    inner: R,
    policy: SecurityPolicy,
}

impl<R: LuaRuntime> SandboxedRuntime<R> {
    pub fn new(inner: R, policy: SecurityPolicy) -> Self {
        Self { inner, policy }
    }
}

impl<R: LuaRuntime> LuaRuntime for SandboxedRuntime<R> {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.policy.can_read(path)?;
        self.inner.file_read(path)
    }

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.policy.can_write(path)?;
        self.inner.file_write(path, contents)
    }

    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        self.policy.can_run(command)?;
        self.inner.exec_pipe(command, args, stdin)
    }

    fn exec_command(&self, command: &str, args: &[&str], stdin: Option<&[u8]>) -> RuntimeResult<CommandOutput> {
        self.policy.can_run(command)?;
        self.inner.exec_command(command, args, stdin)
    }

    fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        // Extract host from URL for permission check
        let host = extract_host_from_url(url);
        self.policy.can_net(&host)?;
        self.inner.fetch_url(url)
    }

    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        self.policy.can_env(name)?;
        self.inner.env_get(name)
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        // Filter to only allowed env vars
        let all = self.inner.env_all()?;
        let filtered: HashMap<_, _> = all.into_iter()
            .filter(|(k, _)| self.policy.can_env(k).is_ok())
            .collect();
        Ok(filtered)
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        self.policy.can_cwd()?;
        self.inner.cwd()
    }

    // Passthrough for safe operations (system info is read-only)
    fn os_name(&self) -> &'static str { self.inner.os_name() }
    fn arch(&self) -> &'static str { self.inner.arch() }

    // ... other delegations with appropriate checks
}

fn extract_host_from_url(url: &str) -> String {
    // Simple extraction - production code would use url crate
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}
```

---

## Lua Standard Library Interception

### Strategy: Replace, Don't Remove

Since we want full Pandoc API compatibility with restrictions, we need to **replace** the standard library functions rather than removing them. This approach:

1. Maintains API compatibility
2. Provides clear error messages
3. Allows restrictions to be runtime-configurable

### Initialization Pattern

```rust
pub fn create_lua_state(runtime: Arc<dyn LuaRuntime>) -> Result<Lua> {
    // Create Lua with minimal libraries (no IO, OS, PACKAGE)
    let lua = Lua::new_with(
        StdLib::COROUTINE
        | StdLib::TABLE
        | StdLib::STRING
        | StdLib::UTF8
        | StdLib::MATH,
        LuaOptions::default()
    )?;

    // Store runtime reference in Lua registry for access from callbacks
    lua.set_app_data(runtime.clone());

    // Install our controlled versions of io/os modules
    install_io_module(&lua, runtime.clone())?;
    install_os_module(&lua, runtime.clone())?;

    // Register pandoc namespace (already exists)
    register_pandoc_namespace(&lua)?;

    // Register pandoc.system and pandoc.path using runtime
    register_pandoc_system(&lua, runtime.clone())?;
    register_pandoc_path(&lua)?;

    Ok(lua)
}
```

### Controlled `io` Module

```rust
fn install_io_module(lua: &Lua, runtime: Arc<dyn LuaRuntime>) -> Result<()> {
    let io = lua.create_table()?;

    // io.open(filename, mode) -> file handle
    let rt = runtime.clone();
    io.set("open", lua.create_function(move |lua, (path, mode): (String, Option<String>)| {
        let mode = mode.unwrap_or_else(|| "r".to_string());
        // Return a file handle userdata that uses runtime for actual I/O
        LuaFile::open(lua, rt.clone(), PathBuf::from(path), &mode)
    })?)?;

    // io.read(...) - Read from stdin (handled by runtime)
    let rt = runtime.clone();
    io.set("read", lua.create_function(move |_, args: MultiValue| {
        // Implementation using runtime
        Err(mlua::Error::runtime("io.read not yet implemented"))
    })?)?;

    // io.write(...) - Write to stdout
    let rt = runtime.clone();
    io.set("write", lua.create_function(move |_, args: MultiValue| {
        for arg in args {
            let s = arg.to_string()?;
            rt.stdout_write(s.as_bytes())
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
        }
        Ok(true)
    })?)?;

    // io.popen - Potentially dangerous, runtime controls
    let rt = runtime.clone();
    io.set("popen", lua.create_function(move |_, (cmd, mode): (String, Option<String>)| {
        // Delegate to runtime which may deny
        Err(mlua::Error::runtime("io.popen not yet implemented"))
    })?)?;

    // io.lines(filename) - Iterator over file lines
    let rt = runtime.clone();
    io.set("lines", lua.create_function(move |lua, path: Option<String>| {
        // Implementation using runtime for file access
        Err(mlua::Error::runtime("io.lines not yet implemented"))
    })?)?;

    lua.globals().set("io", io)?;
    Ok(())
}
```

### Controlled `os` Module

```rust
fn install_os_module(lua: &Lua, runtime: Arc<dyn LuaRuntime>) -> Result<()> {
    let os = lua.create_table()?;

    // os.execute(command) - Dangerous, runtime controls
    let rt = runtime.clone();
    os.set("execute", lua.create_function(move |_, cmd: Option<String>| {
        match cmd {
            None => {
                // os.execute() with no args returns whether shell is available
                Ok((true, "exit", 0))  // Indicate shell exists
            }
            Some(cmd) => {
                // Parse command and execute via runtime
                match rt.exec_command(&cmd, &[], None) {
                    Ok(output) => {
                        if output.code == 0 {
                            Ok((true, "exit", 0))
                        } else {
                            Ok((Value::Nil, "exit", output.code))
                        }
                    }
                    Err(RuntimeError::PermissionDenied(msg)) => {
                        Err(mlua::Error::runtime(msg))
                    }
                    Err(e) => {
                        Err(mlua::Error::runtime(e.to_string()))
                    }
                }
            }
        }
    })?)?;

    // os.getenv(varname)
    let rt = runtime.clone();
    os.set("getenv", lua.create_function(move |_, name: String| {
        rt.env_get(&name)
            .map(|v| v.map(Value::String).unwrap_or(Value::Nil))
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // os.remove(filename)
    let rt = runtime.clone();
    os.set("remove", lua.create_function(move |_, path: String| {
        rt.file_remove(Path::new(&path))
            .map(|_| (true, Value::Nil))
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // os.rename(oldname, newname)
    let rt = runtime.clone();
    os.set("rename", lua.create_function(move |_, (old, new): (String, String)| {
        rt.path_rename(Path::new(&old), Path::new(&new))
            .map(|_| (true, Value::Nil))
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // os.exit([code]) - This one is special
    os.set("exit", lua.create_function(|_, code: Option<i32>| {
        // Don't actually exit! Throw a special error instead
        Err(mlua::Error::runtime(format!(
            "os.exit({}) called - filter terminated",
            code.unwrap_or(0)
        )))
    })?)?;

    // os.tmpname() - Return temp file path
    let rt = runtime.clone();
    os.set("tmpname", lua.create_function(move |_, ()| {
        // Use runtime's temp directory facility
        let temp = rt.temp_dir("lua_")?;
        Ok(temp.path().to_string_lossy().to_string())
    })?)?;

    // Safe functions that don't need runtime
    os.set("clock", lua.create_function(|_, ()| {
        Ok(std::time::Instant::now().elapsed().as_secs_f64())
    })?)?;

    os.set("date", lua.create_function(|_, (format, time): (Option<String>, Option<i64>)| {
        // Standard date formatting
        Err(mlua::Error::runtime("os.date not yet implemented"))
    })?)?;

    os.set("difftime", lua.create_function(|_, (t2, t1): (i64, i64)| {
        Ok(t2 - t1)
    })?)?;

    os.set("time", lua.create_function(|_, table: Option<Table>| {
        // Return current time or from table
        Err(mlua::Error::runtime("os.time not yet implemented"))
    })?)?;

    lua.globals().set("os", os)?;
    Ok(())
}
```

---

## Integration with Pandoc API

### pandoc.system Module

```rust
pub fn register_pandoc_system(lua: &Lua, runtime: Arc<dyn LuaRuntime>) -> Result<()> {
    let system = lua.create_table()?;

    // === Read-only fields ===
    system.set("os", runtime.os_name())?;
    system.set("arch", runtime.arch())?;

    // === File operations ===

    let rt = runtime.clone();
    system.set("read_file", lua.create_function(move |_, path: String| {
        rt.file_read_string(Path::new(&path))
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    let rt = runtime.clone();
    system.set("write_file", lua.create_function(move |_, (path, contents): (String, String)| {
        rt.file_write(Path::new(&path), contents.as_bytes())
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // === Directory operations ===

    let rt = runtime.clone();
    system.set("get_working_directory", lua.create_function(move |_, ()| {
        rt.cwd()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    let rt = runtime.clone();
    system.set("list_directory", lua.create_function(move |_, path: Option<String>| {
        let path = path.unwrap_or_else(|| ".".to_string());
        rt.dir_list(Path::new(&path))
            .map(|entries| {
                entries.into_iter()
                    .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
                    .collect::<Vec<_>>()
            })
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    let rt = runtime.clone();
    system.set("make_directory", lua.create_function(move |_, (path, create_parent): (String, Option<bool>)| {
        let recursive = create_parent.unwrap_or(false);
        rt.dir_create(Path::new(&path), recursive)
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    let rt = runtime.clone();
    system.set("remove_directory", lua.create_function(move |_, (path, recursive): (String, Option<bool>)| {
        let recursive = recursive.unwrap_or(false);
        rt.dir_remove(Path::new(&path), recursive)
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // === Context managers ===

    let rt = runtime.clone();
    system.set("with_working_directory", lua.create_function(move |lua, (path, callback): (String, Function)| {
        // Save current directory
        let original = rt.cwd()
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        // Change to new directory
        // Note: This is tricky because std::env::set_current_dir is process-global
        // We might need to track CWD separately in the runtime

        // Execute callback
        let result = callback.call::<MultiValue>(())?;

        // Restore original directory (even on error, use scope guard)

        Ok(result)
    })?)?;

    let rt = runtime.clone();
    system.set("with_temporary_directory", lua.create_function(move |lua, (parent, template, callback): (Option<String>, String, Function)| {
        // Create temp directory
        let temp = rt.temp_dir(&template)
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        // Execute callback with temp directory path
        let path = temp.path().to_string_lossy().to_string();
        let result = callback.call::<MultiValue>(path)?;

        // Temp directory is cleaned up when `temp` drops

        Ok(result)
    })?)?;

    // === Process execution ===

    let rt = runtime.clone();
    system.set("command", lua.create_function(move |_, (cmd, args, input, opts): (String, Vec<String>, Option<String>, Option<Table>)| {
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let input_bytes = input.map(|s| s.into_bytes());

        match rt.exec_command(&cmd, &args_refs, input_bytes.as_deref()) {
            Ok(output) => {
                let success = output.code == 0;
                Ok((
                    if success { Value::Boolean(false) } else { Value::Integer(output.code as i64) },
                    String::from_utf8_lossy(&output.stdout).to_string(),
                    String::from_utf8_lossy(&output.stderr).to_string(),
                ))
            }
            Err(e) => Err(mlua::Error::runtime(e.to_string())),
        }
    })?)?;

    // === System info ===

    let rt = runtime.clone();
    system.set("cputime", lua.create_function(move |_, ()| {
        rt.cpu_time()
            .map(|t| t as i64)
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    let rt = runtime.clone();
    system.set("environment", lua.create_function(move |lua, ()| {
        let env = rt.env_all()
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        let table = lua.create_table()?;
        for (k, v) in env {
            table.set(k, v)?;
        }
        Ok(table)
    })?)?;

    // Register in pandoc namespace
    let pandoc: Table = lua.globals().get("pandoc")?;
    pandoc.set("system", system)?;

    Ok(())
}
```

### pandoc.path Module

The path module is mostly safe (string manipulation), but `exists` needs runtime:

```rust
pub fn register_pandoc_path(lua: &Lua, runtime: Arc<dyn LuaRuntime>) -> Result<()> {
    let path = lua.create_table()?;

    // Read-only fields
    path.set("separator", std::path::MAIN_SEPARATOR.to_string())?;
    path.set("search_path_separator", if cfg!(windows) { ";" } else { ":" })?;

    // Pure path manipulation (no runtime needed)
    path.set("directory", lua.create_function(|_, filepath: String| {
        let p = Path::new(&filepath);
        Ok(p.parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default())
    })?)?;

    path.set("filename", lua.create_function(|_, filepath: String| {
        let p = Path::new(&filepath);
        Ok(p.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default())
    })?)?;

    path.set("join", lua.create_function(|_, parts: Vec<String>| {
        let mut result = PathBuf::new();
        for part in parts {
            result.push(part);
        }
        Ok(result.to_string_lossy().to_string())
    })?)?;

    path.set("normalize", lua.create_function(|_, filepath: String| {
        // Normalize path separators and remove redundant components
        let p = Path::new(&filepath);
        let mut result = PathBuf::new();
        for component in p.components() {
            result.push(component);
        }
        if result.as_os_str().is_empty() {
            Ok(".".to_string())
        } else {
            Ok(result.to_string_lossy().to_string())
        }
    })?)?;

    path.set("split", lua.create_function(|_, filepath: String| {
        let p = Path::new(&filepath);
        let parts: Vec<String> = p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        Ok(parts)
    })?)?;

    path.set("split_extension", lua.create_function(|_, filepath: String| {
        let p = Path::new(&filepath);
        let stem = p.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = p.extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();

        // Reconstruct path without extension
        let base = if let Some(parent) = p.parent() {
            parent.join(&stem).to_string_lossy().to_string()
        } else {
            stem
        };

        Ok((base, ext))
    })?)?;

    path.set("is_absolute", lua.create_function(|_, filepath: String| {
        Ok(Path::new(&filepath).is_absolute())
    })?)?;

    path.set("is_relative", lua.create_function(|_, filepath: String| {
        Ok(Path::new(&filepath).is_relative())
    })?)?;

    // This one needs runtime for filesystem access
    let rt = runtime.clone();
    path.set("exists", lua.create_function(move |_, (filepath, kind): (String, Option<String>)| {
        let path_kind = kind.as_deref().map(|k| match k {
            "file" => Some(PathKind::File),
            "directory" => Some(PathKind::Directory),
            "symlink" => Some(PathKind::Symlink),
            _ => None,
        }).flatten();

        rt.path_exists(Path::new(&filepath), path_kind)
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // Register in pandoc namespace
    let pandoc: Table = lua.globals().get("pandoc")?;
    pandoc.set("path", path)?;

    Ok(())
}
```

---

## Integration with FilterContext

Update the existing `FilterContext` to carry the runtime:

```rust
// In filter.rs

pub struct FilterContext {
    pub format: String,
    pub input_file: Option<PathBuf>,
    pub resource_path: Vec<PathBuf>,
    pub mediabag: MediaBag,
    pub runtime: Arc<dyn LuaRuntime>,  // NEW
}

impl FilterContext {
    /// Create context for trusted filter execution (full permissions)
    pub fn trusted(format: &str) -> Self {
        Self {
            format: format.to_string(),
            input_file: None,
            resource_path: vec![],
            mediabag: MediaBag::new(),
            runtime: Arc::new(NativeRuntime::default()),
        }
    }

    /// Create context for sandboxed filter execution
    pub fn sandboxed(format: &str, project_root: PathBuf) -> Self {
        let policy = SecurityPolicy::untrusted(project_root);
        let runtime = SandboxedRuntime::new(NativeRuntime::default(), policy);

        Self {
            format: format.to_string(),
            input_file: None,
            resource_path: vec![],
            mediabag: MediaBag::new(),
            runtime: Arc::new(runtime),
        }
    }
}

// Updated apply_lua_filter signature
pub fn apply_lua_filter(
    pandoc: &Pandoc,
    filter_path: &Path,
    context: &FilterContext,
) -> FilterResult<(Pandoc, Vec<DiagnosticMessage>)> {
    let filter_source = std::fs::read_to_string(filter_path)
        .map_err(|e| LuaFilterError::FileReadError(filter_path.to_owned(), e))?;

    // Create Lua state with controlled libraries and runtime
    let lua = create_lua_state(context.runtime.clone())?;

    // ... rest of implementation
}
```

---

## WASM-Specific Considerations

### Async Operations

WASM network operations (`fetch()`) are inherently async, but Lua is synchronous. Options:

1. **Block on async**: Use `wasm-bindgen-futures` to block
2. **Callback-based**: Require filters to use callbacks for network
3. **Coroutine-based**: Yield Lua coroutine during async ops

Recommend: Option 1 for simplicity, with documentation that network ops may be slow.

### Virtual Filesystem

For WASM, we need a virtual filesystem:

```rust
#[cfg(target_arch = "wasm32")]
pub struct VirtualFileSystem {
    /// In-memory file storage
    files: HashMap<PathBuf, Vec<u8>>,

    /// Pre-populated from document's mediabag
    mediabag: MediaBag,
}

impl VirtualFileSystem {
    pub fn new(mediabag: MediaBag) -> Self {
        Self {
            files: HashMap::new(),
            mediabag,
        }
    }

    pub fn read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        // First check in-memory files
        if let Some(content) = self.files.get(path) {
            return Ok(content.clone());
        }

        // Then check mediabag
        let path_str = path.to_string_lossy();
        if let Some((_, content)) = self.mediabag.lookup(&path_str) {
            return Ok(content);
        }

        Err(RuntimeError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("File not found: {}", path.display())
        )))
    }

    pub fn write(&mut self, path: &Path, content: Vec<u8>) -> RuntimeResult<()> {
        self.files.insert(path.to_path_buf(), content);
        Ok(())
    }
}
```

### Feature Flags

```toml
[features]
default = ["native-runtime"]

# Native runtime using std
native-runtime = []

# WASM runtime using Emscripten/web APIs
wasm-runtime = ["wasm-bindgen", "web-sys", "js-sys"]

# Network support (adds reqwest dependency for native)
network = ["reqwest"]
```

---

## Print/Logging Capture

### OutputCapture Trait

```rust
/// Trait for capturing filter output
pub trait OutputCapture: Send + Sync {
    /// Handle print() calls
    fn on_print(&self, message: &str);

    /// Handle stdout writes
    fn on_stdout(&self, data: &[u8]);

    /// Handle stderr writes
    fn on_stderr(&self, data: &[u8]);
}

/// Default: write to actual stdout/stderr
pub struct StdOutputCapture;

impl OutputCapture for StdOutputCapture {
    fn on_print(&self, message: &str) {
        println!("{}", message);
    }

    fn on_stdout(&self, data: &[u8]) {
        use std::io::Write;
        std::io::stdout().write_all(data).ok();
    }

    fn on_stderr(&self, data: &[u8]) {
        use std::io::Write;
        std::io::stderr().write_all(data).ok();
    }
}

/// Capture to in-memory buffer
pub struct BufferOutputCapture {
    stdout: Mutex<Vec<u8>>,
    stderr: Mutex<Vec<u8>>,
}

impl OutputCapture for BufferOutputCapture {
    fn on_print(&self, message: &str) {
        self.stdout.lock().unwrap().extend(message.as_bytes());
        self.stdout.lock().unwrap().push(b'\n');
    }

    fn on_stdout(&self, data: &[u8]) {
        self.stdout.lock().unwrap().extend(data);
    }

    fn on_stderr(&self, data: &[u8]) {
        self.stderr.lock().unwrap().extend(data);
    }
}

/// Write to log file
pub struct FileOutputCapture {
    log_path: PathBuf,
    file: Mutex<std::fs::File>,
}
```

Update `LuaRuntime` to use `OutputCapture`:

```rust
pub trait LuaRuntime: Send + Sync {
    // ... existing methods ...

    /// Get output capture handler
    fn output(&self) -> &dyn OutputCapture;
}
```

---

## Implementation Phases

### Phase 1: Core Abstraction (This Issue - k-475)

1. Define `LuaRuntime` trait with all methods
2. Implement `NativeRuntime` using std
3. Implement `RuntimeError` type
4. Create `SecurityPolicy` and `SandboxedRuntime`
5. Update `create_lua_state` to use `Lua::new_with()`
6. Write comprehensive tests for sandboxing

### Phase 2: Lua Standard Library Replacement

1. Implement controlled `io` module
2. Implement controlled `os` module
3. Disable `package` module (no arbitrary module loading)
4. Add `print()` interception

### Phase 3: Pandoc API Integration

1. Implement `pandoc.system` module using runtime
2. Implement `pandoc.path` module
3. Update `pandoc.mediabag.fetch` to use runtime
4. Connect `pandoc.pipe` to runtime

### Phase 4: WASM Support (Future - separate issue)

1. Implement `WasmRuntime`
2. Implement `VirtualFileSystem`
3. Implement `wasm_fetch` using web APIs
4. Add WASM-specific tests

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_native_runtime_file_read_write() {
        let temp = TempDir::new().unwrap();
        let runtime = NativeRuntime::default();

        let path = temp.path().join("test.txt");
        runtime.file_write(&path, b"hello").unwrap();

        let content = runtime.file_read(&path).unwrap();
        assert_eq!(content, b"hello");
    }

    #[test]
    fn test_sandboxed_runtime_blocks_outside_paths() {
        let temp = TempDir::new().unwrap();
        let policy = SecurityPolicy {
            readable_paths: vec![temp.path().to_path_buf()],
            writable_paths: vec![],
            ..Default::default()
        };

        let runtime = SandboxedRuntime::new(NativeRuntime::default(), policy);

        // Reading inside allowed path should work
        std::fs::write(temp.path().join("test.txt"), "hello").unwrap();
        assert!(runtime.file_read(&temp.path().join("test.txt")).is_ok());

        // Reading outside should fail
        assert!(matches!(
            runtime.file_read(Path::new("/etc/passwd")),
            Err(RuntimeError::PathViolation(_))
        ));
    }

    #[test]
    fn test_sandboxed_runtime_blocks_exec() {
        let policy = SecurityPolicy {
            allow_exec: false,
            ..SecurityPolicy::trusted()
        };

        let runtime = SandboxedRuntime::new(NativeRuntime::default(), policy);

        assert!(matches!(
            runtime.exec_pipe("echo", &["hello"], &[]),
            Err(RuntimeError::PermissionDenied(_))
        ));
    }
}
```

### Integration Tests

```rust
#[test]
fn test_lua_filter_with_sandboxed_runtime() {
    let filter = r#"
        function Para(el)
            -- This should fail in sandboxed mode
            local ok, err = pcall(function()
                os.execute("echo hacked")
            end)
            if not ok then
                return pandoc.Para{pandoc.Str("blocked: " .. err)}
            end
            return el
        end
    "#;

    let context = FilterContext::sandboxed("html", std::env::current_dir().unwrap());
    let doc = parse_qmd("Hello world").unwrap();

    let (result, _) = apply_lua_filter_string(&doc, filter, &context).unwrap();

    // Verify the command was blocked
    let text = stringify(&result);
    assert!(text.contains("blocked:"));
}
```

---

## Design Decisions (Resolved)

The following questions have been resolved based on industry research and project requirements:

### 1. Current Working Directory Handling

**Decision**: CWD is controlled via `allow_cwd` flag in the "env" permission category.

| Runtime | CWD Behavior |
|---------|--------------|
| `NativeRuntime` | Full access (uses `std::env::set_current_dir`) |
| `WasmRuntime` | Not supported (returns `NotSupported` error) |
| `SandboxedRuntime` | Controlled by `allow_cwd` flag |

**Rationale**:
- In Deno, `chdir()` requires `--allow-read` permission ([source](https://docs.deno.com/api/deno/~/Deno.chdir))
- CWD is process-global state, similar to environment variables
- Grouping CWD with env access (under the "env" category) is conceptually clean

**Implementation Note**: The `with_working_directory` function should:
1. Check `can_cwd()` permission
2. Use a per-runtime tracked CWD rather than `std::env::set_current_dir` to avoid process-global mutation
3. Rewrite relative paths in file operations to be relative to the tracked CWD

### 2. Async Network in Sync Lua

**Decision**: Design for **blocking execution** now; note for future async consideration.

**Current approach**:
- Native: Use `reqwest::blocking::Client`
- WASM: Use `wasm-bindgen-futures` to block on `fetch()` promise

**Future consideration**: If/when we need async filters:
- mlua supports async via `AsyncThread` (Lua coroutines + Rust futures)
- Could yield Lua coroutine during network calls
- Would require async-aware filter execution engine

**Note**: This is purely an implementation detail. The `LuaRuntime` trait API is sync-only, which is fine for now.

### 3. Module Loading (`require()`)

**Decision**: Module loading is subject to **file read permissions**.

Path resolution for `require("foo.bar")`:
1. Convert to path: `foo/bar.lua` (or `foo/bar/init.lua`)
2. Resolve relative to script directory or `package.path`
3. Check `can_read(resolved_path)` before loading

| Runtime | `require()` Behavior |
|---------|---------------------|
| `NativeRuntime` | Full access (standard Lua behavior) |
| `WasmRuntime` | Only from virtual filesystem / mediabag |
| `SandboxedRuntime` | Only from `allow_read` paths |

**Implications**:
- Untrusted filters can only require modules within `allow_read` paths
- C modules (`package.cpath`) should be completely disabled in sandboxed mode
- Built-in modules (if any) should be allowlisted separately

### 4. Error Message Granularity

**Decision**: Provide **detailed error messages**.

**Rationale**:
- Follows Deno's pattern of helpful, actionable errors
- Helps users debug permission issues quickly
- Security through obscurity is not our threat model

**Error format examples**:
```
Permission denied: read access to '/etc/passwd' - path is in deny_read list

Permission not granted: run access to 'rm' - add program name to allow_run in SecurityPolicy

Permission not granted: net access to 'api.evil.com' - add host to allow_net in SecurityPolicy
```

---

## Future Considerations

### Async Runtime (Future Issue)

When WASM network becomes a priority:
- Consider `AsyncLuaRuntime` trait variant
- Use mlua's `AsyncThread` for coroutine-based async
- May need separate `AsyncFilterContext` for async-aware execution

### Permission Auditing (Future Issue)

Following [Deno's permission audit feature](https://docs.deno.com/runtime/fundamentals/security/):
- `QUARTO_AUDIT_PERMISSIONS` env var to enable audit logging
- Log all permission checks (granted/denied) to JSONL file
- Useful for debugging and security auditing

### Per-Package Permissions (Future Issue)

Current design is per-filter. Future consideration:
- Allow permission scoping to specific filter files
- Could use filter path in permission lookup
- Similar to npm package-level permissions discussions

---

## Related Issues

- **k-473**: Pandoc Lua API port plan (parent epic)
- **k-409**: Lua filter support (foundation)
- **Future**: WASM runtime implementation
- **Future**: Untrusted filter execution mode

---

## Appendix: Complete Syscall Inventory

### Lua Standard Library

| Module | Function | Risk | Runtime Method |
|--------|----------|------|----------------|
| io | open | High | file_read/write |
| io | read | Medium | stdin handling |
| io | write | Medium | stdout_write |
| io | popen | Critical | exec_pipe |
| io | lines | High | file_read |
| io | tmpfile | Medium | temp_dir |
| io | close | Low | (handle method) |
| os | execute | Critical | exec_command |
| os | getenv | Medium | env_get |
| os | remove | High | file_remove |
| os | rename | High | path_rename |
| os | tmpname | Low | temp_dir |
| os | exit | Medium | (intercept) |
| os | setlocale | Low | (passthrough) |
| os | clock | Safe | (passthrough) |
| os | date | Safe | (passthrough) |
| os | time | Safe | (passthrough) |
| os | difftime | Safe | (passthrough) |
| package | require | High | (disabled) |
| package | loadlib | Critical | (disabled) |
| debug | * | Critical | (disabled) |

### Pandoc API

| Module | Function | Risk | Runtime Method |
|--------|----------|------|----------------|
| pandoc | pipe | Critical | exec_pipe |
| pandoc.system | command | Critical | exec_command |
| pandoc.system | read_file | High | file_read |
| pandoc.system | write_file | High | file_write |
| pandoc.system | make_directory | Medium | dir_create |
| pandoc.system | remove_directory | High | dir_remove |
| pandoc.system | list_directory | Medium | dir_list |
| pandoc.system | get_working_directory | Low | cwd |
| pandoc.system | with_working_directory | Medium | (special) |
| pandoc.system | with_temporary_directory | Medium | temp_dir |
| pandoc.system | copy | High | file_copy |
| pandoc.system | rename | High | path_rename |
| pandoc.system | remove | High | file_remove |
| pandoc.system | times | Low | path_metadata |
| pandoc.system | environment | Medium | env_all |
| pandoc.system | with_environment | Medium | (special) |
| pandoc.system | xdg | Low | xdg_dir |
| pandoc.system | os | Safe | os_name |
| pandoc.system | arch | Safe | arch |
| pandoc.system | cputime | Safe | cpu_time |
| pandoc.path | exists | Low | path_exists |
| pandoc.path | * (others) | Safe | (pure functions) |
| pandoc.mediabag | fetch | High | fetch_url |
| pandoc.mediabag | * (others) | Low | (memory only) |
