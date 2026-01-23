/*
 * traits.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * Defines the LuaRuntime trait and supporting types for the runtime abstraction layer.
 *
 * This abstraction allows code to run in different execution environments:
 * - NativeRuntime: Full system access using std
 * - WasmRuntime: Browser environment with VFS and fetch()
 * - SandboxedRuntime: Restricted access for untrusted code
 */

use async_trait::async_trait;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Result type for runtime operations
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Errors that can occur during runtime operations
#[derive(Debug)]
pub enum RuntimeError {
    /// Standard I/O error
    Io(io::Error),

    /// Permission denied (with detailed reason)
    PermissionDenied(String),

    /// Operation not supported on this runtime (e.g., exec on WASM)
    NotSupported(String),

    /// Path is outside allowed boundary (sandboxing violation)
    PathViolation(PathBuf),

    /// Network operation failed
    Network(String),

    /// Process execution failed
    ProcessFailed {
        /// Exit code (non-zero)
        code: i32,
        /// Error message (usually from stderr)
        message: String,
    },

    /// SASS compilation failed
    SassError(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::Io(e) => write!(f, "I/O error: {}", e),
            RuntimeError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            RuntimeError::NotSupported(msg) => write!(f, "Operation not supported: {}", msg),
            RuntimeError::PathViolation(path) => {
                write!(f, "Path outside allowed boundary: {}", path.display())
            }
            RuntimeError::Network(msg) => write!(f, "Network error: {}", msg),
            RuntimeError::ProcessFailed { code, message } => {
                write!(f, "Process execution failed (exit {}): {}", code, message)
            }
            RuntimeError::SassError(msg) => write!(f, "SASS compilation error: {}", msg),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RuntimeError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for RuntimeError {
    fn from(e: io::Error) -> Self {
        RuntimeError::Io(e)
    }
}

/// Type of filesystem path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Regular file
    File,
    /// Directory
    Directory,
    /// Symbolic link
    Symlink,
}

/// Metadata about a file or directory
#[derive(Debug, Clone)]
pub struct PathMetadata {
    /// Type of path (file, directory, symlink)
    pub kind: PathKind,
    /// Size in bytes (for files)
    pub size: u64,
    /// Last modification time
    pub modified: Option<SystemTime>,
    /// Last access time
    pub accessed: Option<SystemTime>,
    /// Whether the file is read-only
    pub readonly: bool,
}

/// Output from a command execution
#[derive(Debug, Clone)]
pub struct CommandOutput {
    /// Exit code (0 = success)
    pub code: i32,
    /// Standard output
    pub stdout: Vec<u8>,
    /// Standard error
    pub stderr: Vec<u8>,
}

impl CommandOutput {
    /// Check if the command succeeded (exit code 0)
    pub fn success(&self) -> bool {
        self.code == 0
    }

    /// Get stdout as a string (lossy UTF-8 conversion)
    pub fn stdout_string(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    /// Get stderr as a string (lossy UTF-8 conversion)
    pub fn stderr_string(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// XDG base directory types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XdgDirKind {
    /// User-specific configuration files (~/.config)
    Config,
    /// User-specific data files (~/.local/share)
    Data,
    /// User-specific cache files (~/.cache)
    Cache,
    /// User-specific state files (~/.local/state)
    State,
}

/// RAII guard for a temporary directory that cleans up on drop
pub struct TempDir {
    path: PathBuf,
    /// Whether to delete the directory on drop
    cleanup: bool,
}

impl TempDir {
    /// Create a new TempDir from a path
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            cleanup: true,
        }
    }

    /// Get the path to the temporary directory
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Consume the TempDir without cleaning up
    pub fn into_path(mut self) -> PathBuf {
        self.cleanup = false;
        std::mem::take(&mut self.path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if self.cleanup && self.path.exists() {
            // Best effort cleanup - ignore errors
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// Trait defining all low-level runtime operations.
///
/// Implementations of this trait provide the actual system interaction,
/// allowing for different behavior based on target (native, WASM) or
/// security policy (trusted, sandboxed).
///
/// # Design Philosophy
///
/// This trait follows patterns from established runtime permission systems:
/// - [Deno](https://docs.deno.com/runtime/fundamentals/security/)
/// - [Node.js Permission Model](https://nodejs.org/api/permissions.html)
///
/// Key principles:
/// - Secure by default (no permissions granted without explicit opt-in)
/// - Detailed error messages for debugging
/// - Full Pandoc API compatibility (functions exist but may return errors)
///
/// ## JavaScript Execution Design Principles
///
/// **IMPORTANT: These principles are intentional and must be preserved.**
///
/// 1. **Application-specific entry points, NOT generic `eval()`**
///    - The trait exposes purpose-specific methods like `render_ejs()`, not `eval_js()`
///    - This is intentional for safety, testability, and abstraction
///    - Each method documents what it does and what inputs it accepts
///    - No arbitrary code execution is exposed through the public API
///
/// 2. **Implementation-agnostic trait API**
///    - The trait is defined purely in terms of Rust types (`String`, `serde_json::Value`, etc.)
///    - No deno_core, rusty_v8, or wasm-bindgen types leak into the trait definition
///    - This allows swapping the underlying JS engine without changing the public API
///
/// 3. **Asymmetric implementations are acceptable**
///    - Native: embeds V8 via deno_core (Rust calls into embedded JS)
///    - WASM: calls out to browser JS via wasm-bindgen (Rust calls external JS)
///    - The trait hides this architectural difference from consumers
///
/// ## Async Trait Send Bounds
///
/// On native targets, async methods produce `Send` futures (required for multi-threaded async).
/// On WASM, async methods produce `?Send` futures (JsFuture from wasm-bindgen-futures is not Send).
/// This is handled via conditional compilation of the `async_trait` attribute.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait SystemRuntime: Send + Sync {
    // ═══════════════════════════════════════════════════════════════════════
    // FILE OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    /// Read entire file contents as bytes.
    ///
    /// Corresponds to: `pandoc.system.read_file`, `io.open` (read mode)
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>>;

    /// Read file as string with UTF-8 encoding.
    ///
    /// Default implementation reads bytes and converts to string.
    fn file_read_string(&self, path: &Path) -> RuntimeResult<String> {
        let bytes = self.file_read(path)?;
        String::from_utf8(bytes).map_err(|e| {
            RuntimeError::Io(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid UTF-8 in file: {}", e),
            ))
        })
    }

    /// Write bytes to file (creates or overwrites).
    ///
    /// Corresponds to: `pandoc.system.write_file`, `io.open` (write mode)
    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()>;

    /// Check if path exists, optionally filtering by type.
    ///
    /// Corresponds to: `pandoc.path.exists`
    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool>;

    /// Check if path exists and is a file.
    ///
    /// Convenience method that calls `path_exists` with `PathKind::File`.
    fn is_file(&self, path: &Path) -> RuntimeResult<bool> {
        self.path_exists(path, Some(PathKind::File))
    }

    /// Check if path exists and is a directory.
    ///
    /// Convenience method that calls `path_exists` with `PathKind::Directory`.
    fn is_dir(&self, path: &Path) -> RuntimeResult<bool> {
        self.path_exists(path, Some(PathKind::Directory))
    }

    /// Canonicalize a path (resolve symlinks, make absolute).
    ///
    /// On native platforms, this resolves symlinks and returns an absolute path.
    /// On WASM, this normalizes the path (removes `.` and `..` components) but
    /// cannot resolve symlinks since there's no real filesystem.
    ///
    /// Returns an error if the path doesn't exist (on native) or if the path
    /// is malformed.
    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf>;

    /// Get file/directory metadata.
    ///
    /// Corresponds to: `pandoc.system.times` (partial)
    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata>;

    /// Copy file preserving permissions.
    ///
    /// Corresponds to: `pandoc.system.copy`
    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()>;

    /// Rename/move file or directory.
    ///
    /// Corresponds to: `pandoc.system.rename`, `os.rename`
    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()>;

    /// Delete file.
    ///
    /// Corresponds to: `pandoc.system.remove`, `os.remove`
    fn file_remove(&self, path: &Path) -> RuntimeResult<()>;

    // ═══════════════════════════════════════════════════════════════════════
    // DIRECTORY OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    /// Create directory (optionally with parents).
    ///
    /// Corresponds to: `pandoc.system.make_directory`
    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()>;

    /// Remove directory (optionally with contents).
    ///
    /// Corresponds to: `pandoc.system.remove_directory`
    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()>;

    /// List directory entries (excluding . and ..).
    ///
    /// Corresponds to: `pandoc.system.list_directory`
    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>>;

    /// Get current working directory.
    ///
    /// Corresponds to: `pandoc.system.get_working_directory`
    fn cwd(&self) -> RuntimeResult<PathBuf>;

    /// Create temporary directory with given template prefix.
    ///
    /// Corresponds to: `pandoc.system.with_temporary_directory`
    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir>;

    // ═══════════════════════════════════════════════════════════════════════
    // PROCESS EXECUTION
    // ═══════════════════════════════════════════════════════════════════════

    /// Execute command with stdin input, return stdout.
    ///
    /// This is the `pandoc.pipe` equivalent. Throws error on non-zero exit.
    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>>;

    /// Execute command with full output capture.
    ///
    /// This is the `pandoc.system.command` equivalent.
    /// Returns exit code and both stdout/stderr.
    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput>;

    // ═══════════════════════════════════════════════════════════════════════
    // ENVIRONMENT
    // ═══════════════════════════════════════════════════════════════════════

    /// Get single environment variable.
    ///
    /// Corresponds to: `os.getenv`
    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>>;

    /// Get all environment variables.
    ///
    /// Corresponds to: `pandoc.system.environment`
    fn env_all(&self) -> RuntimeResult<HashMap<String, String>>;

    // ═══════════════════════════════════════════════════════════════════════
    // NETWORK
    // ═══════════════════════════════════════════════════════════════════════

    /// Fetch content from URL.
    ///
    /// Returns (content, mime_type).
    ///
    /// Corresponds to: `pandoc.mediabag.fetch` (for URLs)
    fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)>;

    // ═══════════════════════════════════════════════════════════════════════
    // SYSTEM INFO
    // ═══════════════════════════════════════════════════════════════════════

    /// Operating system identifier.
    ///
    /// Returns: "darwin", "linux", "windows", "wasm", etc.
    ///
    /// Corresponds to: `pandoc.system.os`
    fn os_name(&self) -> &'static str;

    /// Machine architecture.
    ///
    /// Returns: "x86_64", "aarch64", "wasm32", etc.
    ///
    /// Corresponds to: `pandoc.system.arch`
    fn arch(&self) -> &'static str;

    /// CPU time used in picoseconds (if available).
    ///
    /// Corresponds to: `pandoc.system.cputime`
    fn cpu_time(&self) -> RuntimeResult<u64>;

    /// XDG base directory lookup.
    ///
    /// Corresponds to: `pandoc.system.xdg`
    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf>;

    // ═══════════════════════════════════════════════════════════════════════
    // BINARY DISCOVERY
    // ═══════════════════════════════════════════════════════════════════════

    /// Find a binary by checking an environment variable first, then PATH.
    ///
    /// This method is useful for finding external tools like pandoc, sass, etc.
    /// The `env_var` parameter specifies an environment variable that may contain
    /// the path to the binary (e.g., "QUARTO_PANDOC" for pandoc).
    ///
    /// Returns `None` if the binary is not found. In WASM environments,
    /// always returns `None` since external binaries are not available.
    ///
    /// Default implementation checks the environment variable but does not
    /// search PATH. Override in `NativeRuntime` to use `which::which()`.
    fn find_binary(&self, name: &str, env_var: &str) -> Option<PathBuf> {
        // Check environment variable first
        if let Ok(Some(path_str)) = self.env_get(env_var) {
            let path = PathBuf::from(path_str);
            if self.is_file(&path).unwrap_or(false) {
                return Some(path);
            }
        }
        // Default implementation does not search PATH
        // NativeRuntime overrides this to use which::which()
        let _ = name; // Silence unused variable warning
        None
    }

    // ═══════════════════════════════════════════════════════════════════════
    // OUTPUT
    // ═══════════════════════════════════════════════════════════════════════

    /// Handle print() output.
    ///
    /// Default implementation writes to stdout.
    fn print(&self, message: &str) {
        println!("{}", message);
    }

    /// Handle io.write() to stdout.
    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()>;

    /// Handle io.write() to stderr.
    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()>;

    // ═══════════════════════════════════════════════════════════════════════
    // JAVASCRIPT EXECUTION
    //
    // These methods provide JavaScript execution capability for features that
    // require it (e.g., EJS template rendering for project creation).
    //
    // See trait-level documentation for design principles.
    // ═══════════════════════════════════════════════════════════════════════

    /// Check if JavaScript execution is available on this runtime.
    ///
    /// Returns `true` if the runtime supports JavaScript execution,
    /// `false` otherwise. Use this to check before calling JS methods
    /// if you want to provide fallback behavior.
    ///
    /// Default: returns `false` (JS not available).
    fn js_available(&self) -> bool {
        false
    }

    /// Render a simple string template using JavaScript.
    ///
    /// Template format: `"Hello, ${name}!"` with data `{"name": "World"}` → `"Hello, World!"`
    ///
    /// Uses simple `${key}` replacement, NOT full JavaScript template literals.
    /// Keys must be simple identifiers (alphanumeric + underscore).
    ///
    /// # Purpose
    ///
    /// This method is scaffolding for validating JS execution architecture.
    /// It tests the Rust ↔ JS data flow without requiring EJS or complex dependencies.
    /// May be removed or refactored once `render_ejs` is working.
    ///
    /// # Arguments
    ///
    /// * `template` - Template string with `${key}` placeholders
    /// * `data` - JSON object with key-value pairs for substitution
    ///
    /// # Returns
    ///
    /// Rendered string on success, `RuntimeError::NotSupported` if JS is not available.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use serde_json::json;
    ///
    /// let result = runtime.js_render_simple_template(
    ///     "Hello, ${name}! You have ${count} messages.",
    ///     &json!({"name": "Alice", "count": 5})
    /// ).await?;
    /// assert_eq!(result, "Hello, Alice! You have 5 messages.");
    /// ```
    async fn js_render_simple_template(
        &self,
        template: &str,
        data: &serde_json::Value,
    ) -> RuntimeResult<String> {
        let _ = (template, data);
        Err(RuntimeError::NotSupported(
            "JavaScript execution is not available on this runtime".to_string(),
        ))
    }

    /// Render an EJS template with the given data.
    ///
    /// [EJS](https://ejs.co/) is a simple templating language that lets you
    /// generate HTML/text with plain JavaScript. This is used for project
    /// scaffolding (creating `_quarto.yml`, `index.qmd`, etc.).
    ///
    /// # Arguments
    ///
    /// * `template` - EJS template string
    /// * `data` - JSON data to pass to the template
    ///
    /// # Returns
    ///
    /// Rendered string on success, `RuntimeError::NotSupported` if JS/EJS is not available.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use serde_json::json;
    ///
    /// let template = r#"
    /// project:
    ///   title: "<%= title %>"
    ///   type: <%= type %>
    /// "#;
    /// let result = runtime.render_ejs(
    ///     template,
    ///     &json!({"title": "My Website", "type": "website"})
    /// ).await?;
    /// ```
    async fn render_ejs(&self, template: &str, data: &serde_json::Value) -> RuntimeResult<String> {
        let _ = (template, data);
        Err(RuntimeError::NotSupported(
            "EJS rendering is not available on this runtime".to_string(),
        ))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // SASS COMPILATION
    //
    // These methods provide SASS/SCSS compilation capability.
    // - Native: Uses the grass crate (pure Rust, ~2x faster than dart-sass)
    // - WASM: Uses dart-sass via JavaScript bridge (lazy-loaded)
    // ═══════════════════════════════════════════════════════════════════════

    /// Check if SASS compilation is available on this runtime.
    ///
    /// Returns `true` if the runtime supports SASS compilation,
    /// `false` otherwise.
    ///
    /// Default: returns `false` (SASS not available).
    fn sass_available(&self) -> bool {
        false
    }

    /// Get the SASS compiler backend name (for diagnostics).
    ///
    /// Returns the name of the SASS compiler being used, or `None` if
    /// SASS compilation is not available.
    ///
    /// Possible values:
    /// - `"grass"` - Pure Rust compiler (native)
    /// - `"dart-sass"` - Reference implementation via JS (WASM)
    fn sass_compiler_name(&self) -> Option<&'static str> {
        None
    }

    /// Compile SCSS source to CSS.
    ///
    /// # Arguments
    ///
    /// * `scss` - The SCSS source code to compile
    /// * `load_paths` - Directories to search for @use/@import resolution
    /// * `minified` - Whether to produce compressed output
    ///
    /// # Returns
    ///
    /// Compiled CSS string on success, `RuntimeError::SassError` on compilation failure,
    /// or `RuntimeError::NotSupported` if SASS compilation is not available.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let scss = "$primary: blue; .btn { color: $primary; }";
    /// let css = runtime.compile_sass(scss, &[], false).await?;
    /// assert!(css.contains(".btn"));
    /// ```
    async fn compile_sass(
        &self,
        scss: &str,
        load_paths: &[PathBuf],
        minified: bool,
    ) -> RuntimeResult<String> {
        let _ = (scss, load_paths, minified);
        Err(RuntimeError::NotSupported(
            "SASS compilation is not available on this runtime".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_output_helpers() {
        let output = CommandOutput {
            code: 0,
            stdout: b"hello world".to_vec(),
            stderr: b"warning".to_vec(),
        };

        assert!(output.success());
        assert_eq!(output.stdout_string(), "hello world");
        assert_eq!(output.stderr_string(), "warning");
    }

    #[test]
    fn test_command_output_failure() {
        let output = CommandOutput {
            code: 1,
            stdout: vec![],
            stderr: b"error occurred".to_vec(),
        };

        assert!(!output.success());
        assert_eq!(output.stderr_string(), "error occurred");
    }

    #[test]
    fn test_runtime_error_display() {
        let err = RuntimeError::PermissionDenied("read access to /etc/passwd".to_string());
        assert!(err.to_string().contains("Permission denied"));
        assert!(err.to_string().contains("/etc/passwd"));

        let err = RuntimeError::NotSupported("process execution in browser".to_string());
        assert!(err.to_string().contains("not supported"));

        let err = RuntimeError::PathViolation(PathBuf::from("/secret/file"));
        assert!(err.to_string().contains("outside allowed boundary"));

        let err = RuntimeError::ProcessFailed {
            code: 127,
            message: "command not found".to_string(),
        };
        assert!(err.to_string().contains("exit 127"));
    }

    #[test]
    fn test_temp_dir_cleanup() {
        let temp_path = {
            let temp = TempDir::new(std::env::temp_dir().join("test_cleanup_12345"));
            std::fs::create_dir_all(temp.path()).unwrap();
            assert!(temp.path().exists());
            temp.path().to_path_buf()
        };
        // TempDir dropped, should be cleaned up
        assert!(!temp_path.exists());
    }

    #[test]
    fn test_temp_dir_into_path() {
        let temp = TempDir::new(std::env::temp_dir().join("test_into_path_12345"));
        std::fs::create_dir_all(temp.path()).unwrap();
        let path = temp.into_path();
        // Should not be cleaned up because we called into_path
        assert!(path.exists());
        // Clean up manually
        std::fs::remove_dir_all(&path).unwrap();
    }
}
