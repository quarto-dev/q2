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
    CommandOptions, CommandOutput, PathKind, PathMetadata, RuntimeError, RuntimeResult,
    SystemRuntime, TempDir, XdgDirKind,
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

    /// Whether `path` may be read: on the allow list and not on the deny
    /// list (deny wins, as in Deno). The path is normalized lexically
    /// first, so `allowed/../elsewhere/x` is judged as `elsewhere/x` and a
    /// `..` cannot walk out of an allow-listed directory (symlinks are not
    /// resolved here; that needs the filesystem).
    pub fn can_read(&self, path: &Path) -> bool {
        let path = quarto_util::normalize_lexically(path);
        !self.deny_read.iter().any(|p| p.matches(&path))
            && self.allow_read.iter().any(|p| p.matches(&path))
    }

    /// Whether `path` may be written, created, removed or renamed; same
    /// rule as [`can_read`](Self::can_read) over the write lists.
    pub fn can_write(&self, path: &Path) -> bool {
        let path = quarto_util::normalize_lexically(path);
        !self.deny_write.iter().any(|p| p.matches(&path))
            && self.allow_write.iter().any(|p| p.matches(&path))
    }
}

/// Sandboxed runtime that enforces security policies.
///
/// This is a decorator that wraps any SystemRuntime and enforces
/// the configured SecurityPolicy on all operations.
pub struct SandboxedRuntime<R: SystemRuntime> {
    inner: R,
    policy: SecurityPolicy,
}

impl<R: SystemRuntime> SandboxedRuntime<R> {
    /// Create a new SandboxedRuntime wrapping the given runtime.
    pub fn new(inner: R, policy: SecurityPolicy) -> Self {
        Self { inner, policy }
    }

    /// The policy this runtime enforces.
    pub fn policy(&self) -> &SecurityPolicy {
        &self.policy
    }

    fn check_read(&self, path: &Path) -> RuntimeResult<()> {
        if self.policy.can_read(path) {
            Ok(())
        } else {
            Err(RuntimeError::PermissionDenied(format!(
                "read access to {}",
                path.display()
            )))
        }
    }

    fn check_write(&self, path: &Path) -> RuntimeResult<()> {
        if self.policy.can_write(path) {
            Ok(())
        } else {
            Err(RuntimeError::PermissionDenied(format!(
                "write access to {}",
                path.display()
            )))
        }
    }
}

// File-system operations check the policy's read and write lists before
// delegating (bd-76ogq4ew: the Lua loaders read only through the runtime
// they are given, so this is where a capability flag such as
// `--allow-read` takes effect). Network, process, environment and system
// checks are still TODO and delegate unchecked.
//
// Note: Using conditional async_trait to match the trait's Send bounds:
// - Native targets: Send futures required (multi-threaded async)
// - WASM targets: ?Send futures allowed (single-threaded, JsFuture is not Send)
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl<R: SystemRuntime> SystemRuntime for SandboxedRuntime<R> {
    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        self.check_read(path)?;
        self.inner.file_read(path)
    }

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        self.check_write(path)?;
        self.inner.file_write(path, contents)
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        self.check_read(path)?;
        self.inner.path_exists(path, kind)
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        self.check_read(path)?;
        self.inner.canonicalize(path)
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        self.check_read(path)?;
        self.inner.path_metadata(path)
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        self.check_read(src)?;
        self.check_write(dst)?;
        self.inner.file_copy(src, dst)
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        self.check_write(old)?;
        self.check_write(new)?;
        self.inner.path_rename(old, new)
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        self.check_write(path)?;
        self.inner.file_remove(path)
    }

    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.check_write(path)?;
        self.inner.dir_create(path, recursive)
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        self.check_write(path)?;
        self.inner.dir_remove(path, recursive)
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        self.check_read(path)?;
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

    fn exec_command_with_options(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
        options: &CommandOptions<'_>,
    ) -> RuntimeResult<CommandOutput> {
        self.inner
            .exec_command_with_options(command, args, stdin, options)
    }

    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        // TODO: Check policy.can_env(name)
        self.inner.env_get(name)
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        self.inner.env_all()
    }

    async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        // TODO: Check policy.can_net(host)
        self.inner.fetch_url(url).await
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

    fn unix_timestamp(&self) -> RuntimeResult<u64> {
        self.inner.unix_timestamp()
    }

    fn local_utc_offset_seconds(&self, unix_timestamp: i64) -> i32 {
        self.inner.local_utc_offset_seconds(unix_timestamp)
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

    fn runtime_metadata(&self) -> Option<serde_json::Value> {
        self.inner.runtime_metadata()
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

    /// A relative traversal spelled under an allowed directory is judged
    /// by where it lands.
    #[test]
    fn a_dot_dot_cannot_escape_the_allow_list() {
        let policy = SecurityPolicy::untrusted(PathBuf::from("/proj"));
        assert!(policy.can_read(Path::new("/proj/sub/../a.md")));
        assert!(!policy.can_read(Path::new("/proj/../vendor/secret.md")));
        assert!(!policy.can_write(Path::new("/proj/./../vendor/out.md")));
    }

    #[test]
    fn can_read_is_allow_minus_deny() {
        let mut policy = SecurityPolicy::untrusted(PathBuf::from("/proj"));
        assert!(policy.can_read(Path::new("/proj/a/b.lmd")));
        assert!(!policy.can_read(Path::new("/elsewhere/b.lmd")));
        policy.deny_read.push(PathPattern::new("/proj/secret/"));
        assert!(!policy.can_read(Path::new("/proj/secret/key.lua")));
        assert!(policy.can_read(Path::new("/proj/open/key.lua")));
    }

    #[test]
    fn sandboxed_runtime_refuses_reads_outside_the_policy() {
        use crate::native::NativeRuntime;
        let tmp = tempfile::TempDir::new().unwrap();
        let allowed = tmp.path().join("proj");
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&allowed).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(allowed.join("in.txt"), b"in").unwrap();
        std::fs::write(outside.join("out.txt"), b"out").unwrap();

        let rt = SandboxedRuntime::new(
            NativeRuntime::new(),
            SecurityPolicy::untrusted(allowed.clone()),
        );
        assert_eq!(rt.file_read(&allowed.join("in.txt")).unwrap(), b"in");
        assert!(rt.path_exists(&allowed.join("in.txt"), None).unwrap());

        let err = rt.file_read(&outside.join("out.txt")).unwrap_err();
        assert!(matches!(err, RuntimeError::PermissionDenied(_)), "{err:?}");
        let err = rt.path_exists(&outside.join("out.txt"), None).unwrap_err();
        assert!(matches!(err, RuntimeError::PermissionDenied(_)), "{err:?}");
        // Writing inside the project but outside `_output` is refused too.
        let err = rt.file_write(&allowed.join("w.txt"), b"x").unwrap_err();
        assert!(matches!(err, RuntimeError::PermissionDenied(_)), "{err:?}");
    }

    #[test]
    fn test_security_policy_untrusted() {
        let policy = SecurityPolicy::untrusted(PathBuf::from("/project"));
        assert!(!policy.allow_cwd);
        assert!(policy.allow_run.is_empty());
        assert!(policy.allow_net.is_empty());
    }
}
