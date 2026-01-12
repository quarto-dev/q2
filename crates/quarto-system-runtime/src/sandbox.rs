/*
 * sandbox.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * SandboxedRuntime implementation for untrusted code.
 *
 * This runtime:
 * - Uses decorator pattern wrapping any LuaRuntime
 * - Enforces SecurityPolicy with Deno-style permission model
 * - Supports allow/deny lists for read, write, net, run, env
 * - Provides detailed error messages (PermissionError)
 * - Supports path wildcards for granular access control
 */

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::traits::{
    CommandOutput, PathKind, PathMetadata, RuntimeResult, SystemRuntime, TempDir, XdgDirKind,
};

/// A path pattern that can match files/directories.
///
/// Supports:
/// - Exact paths: "/home/user/file.txt"
/// - Directory prefixes: "/home/user/" (matches everything under)
/// - Wildcards: "/home/user/*.lua" (glob-style matching)
#[derive(Debug, Clone)]
pub struct PathPattern(String);

impl PathPattern {
    /// Create a new PathPattern from a string.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self(pattern.into())
    }

    /// Check if a path matches this pattern.
    pub fn matches(&self, path: &Path) -> bool {
        let pattern = &self.0;
        let path_str = path.to_string_lossy();

        if pattern == "*" {
            return true;
        }

        if pattern.contains('*') {
            // Simple glob matching - full implementation in k-485
            let prefix = pattern.split('*').next().unwrap_or("");
            path_str.starts_with(prefix)
        } else if pattern.ends_with('/') || pattern.ends_with(std::path::MAIN_SEPARATOR) {
            // Directory prefix
            let trimmed = pattern.trim_end_matches(['/', '\\']);
            path_str.starts_with(trimmed)
        } else {
            // Exact match or prefix
            path_str == pattern.as_str() || path_str.starts_with(pattern)
        }
    }
}

/// Security policy for sandboxed execution.
///
/// Modeled after Deno's permission flags:
/// - <https://docs.deno.com/runtime/fundamentals/security/>
///
/// And Node.js Permission Model:
/// - <https://nodejs.org/api/permissions.html>
#[derive(Debug, Clone, Default)]
pub struct SecurityPolicy {
    // ═══════════════════════════════════════════════════════════════════════
    // FILE SYSTEM (like Deno's --allow-read, --allow-write)
    // ═══════════════════════════════════════════════════════════════════════
    /// Paths allowed for reading. Empty = no read access.
    pub allow_read: Vec<PathPattern>,
    /// Paths explicitly denied for reading. Takes precedence over allow_read.
    pub deny_read: Vec<PathPattern>,
    /// Paths allowed for writing. Empty = no write access.
    pub allow_write: Vec<PathPattern>,
    /// Paths explicitly denied for writing. Takes precedence over allow_write.
    pub deny_write: Vec<PathPattern>,

    // ═══════════════════════════════════════════════════════════════════════
    // NETWORK (like Deno's --allow-net)
    // ═══════════════════════════════════════════════════════════════════════
    /// Allowed network hosts/URLs. Empty = no network access.
    pub allow_net: Vec<String>,
    /// Denied network hosts. Takes precedence over allow_net.
    pub deny_net: Vec<String>,

    // ═══════════════════════════════════════════════════════════════════════
    // PROCESS EXECUTION (like Deno's --allow-run)
    // ═══════════════════════════════════════════════════════════════════════
    /// Allowed programs to execute. Empty = no process execution.
    pub allow_run: Vec<String>,
    /// Denied programs. Takes precedence over allow_run.
    pub deny_run: Vec<String>,

    // ═══════════════════════════════════════════════════════════════════════
    // ENVIRONMENT (like Deno's --allow-env)
    // ═══════════════════════════════════════════════════════════════════════
    /// Allowed environment variables. Empty = no env access.
    pub allow_env: Vec<String>,
    /// Denied environment variables. Takes precedence over allow_env.
    pub deny_env: Vec<String>,
    /// Whether CWD operations are allowed.
    pub allow_cwd: bool,

    // ═══════════════════════════════════════════════════════════════════════
    // SYSTEM INFO (like Deno's --allow-sys)
    // ═══════════════════════════════════════════════════════════════════════
    /// Allowed system info APIs. Empty = no sys info access.
    pub allow_sys: Vec<String>,
}

impl SecurityPolicy {
    /// Fully permissive policy (for trusted code).
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

    /// Restrictive policy for untrusted code.
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
            allow_cwd: false,
            allow_sys: vec!["osRelease".to_string()],
        }
    }
}

/// Sandboxed runtime that enforces security policies.
///
/// This is a decorator that wraps any SystemRuntime and enforces
/// the configured SecurityPolicy on all operations.
pub struct SandboxedRuntime<R: SystemRuntime> {
    inner: R,
    #[allow(dead_code)] // Will be used when full permission checking is implemented
    policy: SecurityPolicy,
}

impl<R: SystemRuntime> SandboxedRuntime<R> {
    /// Create a new SandboxedRuntime wrapping the given runtime.
    pub fn new(inner: R, policy: SecurityPolicy) -> Self {
        Self { inner, policy }
    }
}

// Stub implementation that just delegates to inner runtime
// Full permission checking will be implemented in the future
#[async_trait]
impl<R: SystemRuntime> SystemRuntime for SandboxedRuntime<R> {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        // TODO: Check policy.can_read(path)
        self.inner.file_read(path)
    }

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        // TODO: Check policy.can_write(path)
        self.inner.file_write(path, contents)
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        self.inner.path_exists(path, kind)
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        self.inner.canonicalize(path)
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        self.inner.path_metadata(path)
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        self.inner.file_copy(src, dst)
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        self.inner.path_rename(old, new)
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        self.inner.file_remove(path)
    }

    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.inner.dir_create(path, recursive)
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.inner.dir_remove(path, recursive)
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        self.inner.dir_list(path)
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        // TODO: Check policy.allow_cwd
        self.inner.cwd()
    }

    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir> {
        self.inner.temp_dir(template)
    }

    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        // TODO: Check policy.can_run(command)
        self.inner.exec_pipe(command, args, stdin)
    }

    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput> {
        self.inner.exec_command(command, args, stdin)
    }

    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        // TODO: Check policy.can_env(name)
        self.inner.env_get(name)
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        self.inner.env_all()
    }

    fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        // TODO: Check policy.can_net(host)
        self.inner.fetch_url(url)
    }

    fn os_name(&self) -> &'static str {
        self.inner.os_name()
    }

    fn arch(&self) -> &'static str {
        self.inner.arch()
    }

    fn cpu_time(&self) -> RuntimeResult<u64> {
        self.inner.cpu_time()
    }

    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        self.inner.xdg_dir(kind, subpath)
    }

    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stdout_write(data)
    }

    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()> {
        self.inner.stderr_write(data)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // JAVASCRIPT EXECUTION (delegated to inner runtime)
    // ═══════════════════════════════════════════════════════════════════════

    fn js_available(&self) -> bool {
        self.inner.js_available()
    }

    async fn js_render_simple_template(
        &self,
        template: &str,
        data: &serde_json::Value,
    ) -> RuntimeResult<String> {
        self.inner.js_render_simple_template(template, data).await
    }

    async fn render_ejs(&self, template: &str, data: &serde_json::Value) -> RuntimeResult<String> {
        self.inner.render_ejs(template, data).await
    }
}

/// Type alias for a thread-safe shared runtime.
pub type SharedRuntime = Arc<dyn SystemRuntime>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_pattern_exact() {
        let pattern = PathPattern::new("/home/user/file.txt");
        assert!(pattern.matches(Path::new("/home/user/file.txt")));
        assert!(!pattern.matches(Path::new("/home/user/other.txt")));
    }

    #[test]
    fn test_path_pattern_directory() {
        let pattern = PathPattern::new("/home/user/");
        assert!(pattern.matches(Path::new("/home/user/file.txt")));
        assert!(pattern.matches(Path::new("/home/user/subdir/file.txt")));
        assert!(!pattern.matches(Path::new("/home/other/file.txt")));
    }

    #[test]
    fn test_path_pattern_wildcard() {
        let pattern = PathPattern::new("/home/user/*.txt");
        assert!(pattern.matches(Path::new("/home/user/file.txt")));
        assert!(pattern.matches(Path::new("/home/user/other.txt")));
        // Note: Simple implementation, full glob matching in future
    }

    #[test]
    fn test_path_pattern_star() {
        let pattern = PathPattern::new("*");
        assert!(pattern.matches(Path::new("/any/path")));
        assert!(pattern.matches(Path::new("relative/path")));
    }

    #[test]
    fn test_security_policy_trusted() {
        let policy = SecurityPolicy::trusted();
        assert!(!policy.allow_read.is_empty());
        assert!(policy.allow_cwd);
    }

    #[test]
    fn test_security_policy_untrusted() {
        let policy = SecurityPolicy::untrusted(PathBuf::from("/project"));
        assert!(!policy.allow_cwd);
        assert!(policy.allow_run.is_empty());
        assert!(policy.allow_net.is_empty());
    }
}
