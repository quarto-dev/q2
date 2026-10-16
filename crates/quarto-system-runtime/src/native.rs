/*
 * native.rs
 * Copyright (c) 2025 Posit, PBC
 *
 * NativeRuntime implementation.
 *
 * This runtime provides full system access using std:
 * - std::fs for file operations
 * - std::process for command execution
 * - std::env for environment access
 *
 * This is the default runtime for native (non-WASM) targets.
 */

use async_trait::async_trait;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::sass_native;
use std::sync::OnceLock;
use std::time::Duration;

use crate::traits::{
    CommandOutput, FetchPolicy, PathKind, PathMetadata, RuntimeError, RuntimeResult, SassOutput,
    SystemRuntime, TempDir, XdgDirKind, validate_fetch_url,
};

/// How long [`SystemRuntime::fetch_url`] waits for a whole request, headers
/// and body, before giving up. reqwest has no timeout by default, so without
/// this a stalled connection would hang a render (or a CI run) indefinitely.
/// Matches `quarto-source-fetch`'s request timeout.
pub const FETCH_URL_TIMEOUT: Duration = Duration::from_secs(30);

/// The blocking HTTP client behind [`SystemRuntime::fetch_url`].
///
/// Blocking rather than async: the native render pipeline is driven by
/// `pollster::block_on` (render_to_file.rs, pipeline.rs tests), which
/// provides no tokio reactor, and async reqwest panics with "there is no
/// reactor running". Blocking is appropriate: Lua filter execution already
/// occupies a dedicated thread, and fetches happen only when a document or
/// filter explicitly asks for a URL. One client is shared so its connection
/// pool and TLS configuration are built once.
fn http_client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .timeout(FETCH_URL_TIMEOUT)
            .build()
            // The builder only fails on an unusable TLS/system configuration;
            // fall back to the default client rather than making every fetch
            // unavailable.
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    })
}

/// GET `url` with `timeout` covering the whole request; returns the body and
/// its `content-type`.
///
/// A non-2xx status is an error. A 404 or 503 body is an HTML error page,
/// not the resource: handing it back as success gives callers something that
/// parses as nothing (a font stylesheet with no `@font-face`, an image that
/// will not decode) with no hint of why. The status code is in the message
/// so a log can say what the server actually answered.
fn fetch_blocking(url: &str, timeout: Duration) -> RuntimeResult<(Vec<u8>, String)> {
    let response = http_client()
        .get(url)
        .timeout(timeout)
        .send()
        .map_err(|e| {
            if e.is_timeout() {
                // Name the timeout: reqwest's Display ("error sending
                // request") never says the request timed out, and a caller
                // debugging a stalled fetch needs that word.
                RuntimeError::Network(format!("HTTP request to {url} timed out after {timeout:?}"))
            } else {
                RuntimeError::Network(format!("HTTP request failed for {url}: {e}"))
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(RuntimeError::Network(format!("HTTP {status} for {url}")));
    }
    let mime_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let content = response.bytes().map_err(|e| {
        RuntimeError::Network(format!("Failed to read response body for {url}: {e}"))
    })?;
    Ok((content.to_vec(), mime_type))
}

/// Native runtime with full system access.
///
/// This is the default runtime for trusted code on native targets.
/// It provides unrestricted access to the filesystem, process execution,
/// environment variables, and network (if enabled).
#[derive(Debug, Default)]
pub struct NativeRuntime {
    /// Optional cache directory for persistent caching.
    /// When `None`, all cache operations are silent no-ops.
    cache_dir: Option<PathBuf>,
    /// Environment reads answered from this instance before the process
    /// environment (`env_get`, `env_all`). Instance-scoped: nothing is
    /// set process-wide, so two runtimes with different overrides in one
    /// process do not see each other's.
    env_overrides: Vec<(String, String)>,
}

impl NativeRuntime {
    /// Create a new NativeRuntime with default settings (no caching).
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a NativeRuntime with a cache directory for persistent caching.
    ///
    /// Typically set to `{project_dir}/.quarto/cache/`.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir: Some(cache_dir),
            ..Self::default()
        }
    }

    /// Answer `env_get(name)` (and `env_all`) with `value` on this runtime
    /// instead of the process environment. A later override of the same
    /// name replaces an earlier one.
    ///
    /// The live preview server uses it to carry `--profile` as
    /// `QUARTO_PROFILE` to project discovery without touching the process
    /// environment.
    #[must_use]
    pub fn with_env_override(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let name = name.into();
        self.env_overrides.retain(|(n, _)| *n != name);
        self.env_overrides.push((name, value.into()));
        self
    }

    /// Returns the configured cache directory, if any.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }
}

#[async_trait]
impl SystemRuntime for NativeRuntime {
    // ═══════════════════════════════════════════════════════════════════════
    // FILE OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    fn file_read(&self, path: &Path) -> RuntimeResult<Vec<u8>> {
        fs::read(path).map_err(RuntimeError::from)
    }

    fn file_write(&self, path: &Path, contents: &[u8]) -> RuntimeResult<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents).map_err(RuntimeError::from)
    }

    fn path_exists(&self, path: &Path, kind: Option<PathKind>) -> RuntimeResult<bool> {
        if !path.exists() {
            return Ok(false);
        }

        match kind {
            None => Ok(true),
            Some(PathKind::File) => Ok(path.is_file()),
            Some(PathKind::Directory) => Ok(path.is_dir()),
            Some(PathKind::Symlink) => Ok(path.symlink_metadata()?.file_type().is_symlink()),
        }
    }

    fn canonicalize(&self, path: &Path) -> RuntimeResult<PathBuf> {
        crate::canonicalize(path).map_err(RuntimeError::from)
    }

    fn path_metadata(&self, path: &Path) -> RuntimeResult<PathMetadata> {
        let metadata = fs::metadata(path)?;
        let file_type = metadata.file_type();

        let kind = if file_type.is_file() {
            PathKind::File
        } else if file_type.is_dir() {
            PathKind::Directory
        } else {
            PathKind::Symlink
        };

        Ok(PathMetadata {
            kind,
            size: metadata.len(),
            modified: metadata.modified().ok(),
            accessed: metadata.accessed().ok(),
            readonly: metadata.permissions().readonly(),
        })
    }

    fn file_copy(&self, src: &Path, dst: &Path) -> RuntimeResult<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = dst.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
        Ok(())
    }

    fn path_rename(&self, old: &Path, new: &Path) -> RuntimeResult<()> {
        // Create parent directories if they don't exist
        if let Some(parent) = new.parent()
            && !parent.exists()
        {
            fs::create_dir_all(parent)?;
        }
        fs::rename(old, new).map_err(RuntimeError::from)
    }

    fn file_remove(&self, path: &Path) -> RuntimeResult<()> {
        fs::remove_file(path).map_err(RuntimeError::from)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // DIRECTORY OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════

    fn dir_create(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        if recursive {
            fs::create_dir_all(path).map_err(RuntimeError::from)
        } else {
            fs::create_dir(path).map_err(RuntimeError::from)
        }
    }

    fn dir_remove(&self, path: &Path, recursive: bool) -> RuntimeResult<()> {
        if recursive {
            fs::remove_dir_all(path).map_err(RuntimeError::from)
        } else {
            fs::remove_dir(path).map_err(RuntimeError::from)
        }
    }

    fn dir_list(&self, path: &Path) -> RuntimeResult<Vec<PathBuf>> {
        let entries: Result<Vec<_>, _> = fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect();
        entries.map_err(RuntimeError::from)
    }

    fn cwd(&self) -> RuntimeResult<PathBuf> {
        std::env::current_dir().map_err(RuntimeError::from)
    }

    fn temp_dir(&self, template: &str) -> RuntimeResult<TempDir> {
        // Use tempfile::Builder for unique directory names that are safe
        // under parallel execution (e.g. cargo nextest). The previous
        // timestamp-based naming could collide across concurrent processes.
        let dir = tempfile::Builder::new()
            .prefix(&format!("{}_", template))
            .tempdir()
            .map_err(RuntimeError::from)?;
        let path = dir.keep(); // take ownership of the path, we manage cleanup via our TempDir
        Ok(TempDir::new(path))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // PROCESS EXECUTION
    // ═══════════════════════════════════════════════════════════════════════

    fn exec_pipe(&self, command: &str, args: &[&str], stdin: &[u8]) -> RuntimeResult<Vec<u8>> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write stdin
        if let Some(mut stdin_pipe) = child.stdin.take() {
            stdin_pipe.write_all(stdin)?;
        }

        let output = child.wait_with_output()?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(RuntimeError::ProcessFailed {
                code: output.status.code().unwrap_or(-1),
                message: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    fn exec_command(
        &self,
        command: &str,
        args: &[&str],
        stdin: Option<&[u8]>,
    ) -> RuntimeResult<CommandOutput> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Write stdin if provided
        if let Some(input) = stdin
            && let Some(mut stdin_pipe) = child.stdin.take()
        {
            stdin_pipe.write_all(input)?;
        }

        let output = child.wait_with_output()?;

        Ok(CommandOutput {
            code: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }

    // ═══════════════════════════════════════════════════════════════════════
    // ENVIRONMENT
    // ═══════════════════════════════════════════════════════════════════════

    fn env_get(&self, name: &str) -> RuntimeResult<Option<String>> {
        if let Some((_, value)) = self.env_overrides.iter().find(|(n, _)| n == name) {
            return Ok(Some(value.clone()));
        }
        Ok(std::env::var(name).ok())
    }

    fn env_all(&self) -> RuntimeResult<HashMap<String, String>> {
        let mut vars: HashMap<String, String> = std::env::vars().collect();
        vars.extend(self.env_overrides.iter().cloned());
        Ok(vars)
    }

    fn is_interactive(&self) -> bool {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // NETWORK
    // ═══════════════════════════════════════════════════════════════════════

    async fn fetch_url(&self, url: &str) -> RuntimeResult<(Vec<u8>, String)> {
        fetch_blocking(url, FETCH_URL_TIMEOUT)
    }

    /// The trait default delegates to `fetch_url`, whose fixed timeout would
    /// ignore the policy's. Enforce the policy's own timeout here, so a
    /// remote-image prefetch gives up when its `FetchPolicy` says, not after
    /// [`FETCH_URL_TIMEOUT`].
    async fn fetch_url_hardened(
        &self,
        url: &str,
        policy: &FetchPolicy,
    ) -> RuntimeResult<(Vec<u8>, String)> {
        validate_fetch_url(url).map_err(RuntimeError::Network)?;
        let (bytes, mime) =
            fetch_blocking(url, Duration::from_millis(u64::from(policy.timeout_ms)))?;
        if bytes.len() as u64 > policy.max_bytes {
            return Err(RuntimeError::Network(format!(
                "{url} is {} bytes; the limit is {}",
                bytes.len(),
                policy.max_bytes
            )));
        }
        Ok((bytes, mime))
    }

    // ═══════════════════════════════════════════════════════════════════════
    // SYSTEM INFO
    // ═══════════════════════════════════════════════════════════════════════

    fn os_name(&self) -> &'static str {
        std::env::consts::OS
    }

    fn arch(&self) -> &'static str {
        std::env::consts::ARCH
    }

    fn cpu_time(&self) -> RuntimeResult<u64> {
        // CPU time is platform-specific and not easily available in std
        // For now, return an approximation based on elapsed time
        // A more accurate implementation would use platform-specific APIs
        Err(RuntimeError::NotSupported(
            "CPU time measurement not available on this platform".to_string(),
        ))
    }

    fn xdg_dir(&self, kind: XdgDirKind, subpath: Option<&Path>) -> RuntimeResult<PathBuf> {
        let base = match kind {
            XdgDirKind::Config => {
                // $XDG_CONFIG_HOME or ~/.config
                std::env::var("XDG_CONFIG_HOME").map_or_else(
                    |_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".config")
                    },
                    PathBuf::from,
                )
            }
            XdgDirKind::Data => {
                // $XDG_DATA_HOME or ~/.local/share
                std::env::var("XDG_DATA_HOME").map_or_else(
                    |_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".local/share")
                    },
                    PathBuf::from,
                )
            }
            XdgDirKind::Cache => {
                // $XDG_CACHE_HOME or ~/.cache
                std::env::var("XDG_CACHE_HOME").map_or_else(
                    |_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".cache")
                    },
                    PathBuf::from,
                )
            }
            XdgDirKind::State => {
                // $XDG_STATE_HOME or ~/.local/state
                std::env::var("XDG_STATE_HOME").map_or_else(
                    |_| {
                        dirs::home_dir()
                            .unwrap_or_else(|| PathBuf::from("~"))
                            .join(".local/state")
                    },
                    PathBuf::from,
                )
            }
        };

        match subpath {
            Some(sub) => Ok(base.join(sub)),
            None => Ok(base),
        }
    }

    // ═══════════════════════════════════════════════════════════════════════
    // BINARY DISCOVERY
    // ═══════════════════════════════════════════════════════════════════════

    fn find_binary(&self, name: &str, env_var: &str) -> Option<PathBuf> {
        // Check environment variable first
        if let Ok(path_str) = std::env::var(env_var) {
            let path = PathBuf::from(path_str);
            if path.is_file() {
                return Some(path);
            }
        }
        // Fall back to PATH lookup
        which::which(name).ok()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // OUTPUT
    // ═══════════════════════════════════════════════════════════════════════

    fn stdout_write(&self, data: &[u8]) -> RuntimeResult<()> {
        io::stdout().write_all(data).map_err(RuntimeError::from)
    }

    fn stderr_write(&self, data: &[u8]) -> RuntimeResult<()> {
        io::stderr().write_all(data).map_err(RuntimeError::from)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // SASS COMPILATION
    //
    // Uses the grass crate for pure Rust SASS compilation.
    // grass targets dart-sass 1.54.3 and is ~2x faster.
    // ═══════════════════════════════════════════════════════════════════════

    fn sass_available(&self) -> bool {
        true // grass is always available on native
    }

    fn sass_compiler_name(&self) -> Option<&'static str> {
        Some("grass")
    }

    async fn compile_sass(
        &self,
        scss: &str,
        load_paths: &[PathBuf],
        minified: bool,
    ) -> RuntimeResult<SassOutput> {
        sass_native::compile_scss(self, scss, load_paths, minified)
    }

    // ═══════════════════════════════════════════════════════════════════════
    // CACHING (filesystem-backed)
    //
    // Layout: {cache_dir}/{namespace}/{key}
    // Atomic writes via tempfile + rename.
    // ═══════════════════════════════════════════════════════════════════════

    async fn cache_get(&self, namespace: &str, key: &str) -> RuntimeResult<Option<Vec<u8>>> {
        let Some(cache_dir) = &self.cache_dir else {
            return Ok(None);
        };
        crate::traits::validate_cache_namespace(namespace)?;
        crate::traits::validate_cache_key(key)?;
        let path = cache_dir.join(namespace).join(key);
        match fs::read(&path) {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(RuntimeError::CacheError(format!(
                "failed to read cache entry {namespace}/{key}: {e}"
            ))),
        }
    }

    async fn cache_set(&self, namespace: &str, key: &str, value: &[u8]) -> RuntimeResult<()> {
        let Some(cache_dir) = &self.cache_dir else {
            return Ok(());
        };
        crate::traits::validate_cache_namespace(namespace)?;
        crate::traits::validate_cache_key(key)?;
        let ns_dir = cache_dir.join(namespace);
        fs::create_dir_all(&ns_dir).map_err(|e| {
            RuntimeError::CacheError(format!(
                "failed to create cache directory {}: {e}",
                ns_dir.display()
            ))
        })?;
        // Atomic write: temp file in same directory, then persist (rename)
        let temp = tempfile::NamedTempFile::new_in(&ns_dir).map_err(|e| {
            RuntimeError::CacheError(format!("failed to create temp file for cache write: {e}"))
        })?;
        fs::write(temp.path(), value).map_err(|e| {
            RuntimeError::CacheError(format!(
                "failed to write cache entry {namespace}/{key}: {e}"
            ))
        })?;
        let target = ns_dir.join(key);
        temp.persist(&target).map_err(|e| {
            RuntimeError::CacheError(format!(
                "failed to persist cache entry {namespace}/{key}: {e}"
            ))
        })?;
        Ok(())
    }

    async fn cache_delete(&self, namespace: &str, key: &str) -> RuntimeResult<()> {
        let Some(cache_dir) = &self.cache_dir else {
            return Ok(());
        };
        crate::traits::validate_cache_namespace(namespace)?;
        crate::traits::validate_cache_key(key)?;
        let path = cache_dir.join(namespace).join(key);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(RuntimeError::CacheError(format!(
                "failed to delete cache entry {namespace}/{key}: {e}"
            ))),
        }
    }

    async fn cache_clear_namespace(&self, namespace: &str) -> RuntimeResult<()> {
        let Some(cache_dir) = &self.cache_dir else {
            return Ok(());
        };
        crate::traits::validate_cache_namespace(namespace)?;
        let ns_dir = cache_dir.join(namespace);
        match fs::remove_dir_all(&ns_dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(RuntimeError::CacheError(format!(
                "failed to clear cache namespace {namespace}: {e}"
            ))),
        }
    }

    async fn cache_list(
        &self,
        namespace: &str,
    ) -> RuntimeResult<Option<Vec<crate::traits::CacheEntryInfo>>> {
        let Some(cache_dir) = &self.cache_dir else {
            return Ok(None);
        };
        crate::traits::validate_cache_namespace(namespace)?;
        let ns_dir = cache_dir.join(namespace);
        let read_dir = match fs::read_dir(&ns_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(Vec::new())),
            Err(e) => {
                return Err(RuntimeError::CacheError(format!(
                    "failed to list cache namespace {namespace}: {e}"
                )));
            }
        };
        let mut entries = Vec::new();
        for entry in read_dir {
            let Ok(entry) = entry else { continue };
            let name = entry.file_name();
            let Some(key) = name.to_str() else { continue };
            // `cache_set` writes through a `NamedTempFile` in the same
            // directory; an in-flight one is not an entry.
            if key.starts_with(".tmp") || crate::traits::validate_cache_key(key).is_err() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified_ms = meta.modified().ok().and_then(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as u64)
            });
            entries.push(crate::traits::CacheEntryInfo {
                key: key.to_string(),
                size: meta.len(),
                modified_ms,
            });
        }
        Ok(Some(entries))
    }
}

// Fallback for xdg_dir when dirs crate is not available
mod dirs {
    use std::path::PathBuf;

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map(PathBuf::from)
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir as TempFileTempDir;

    fn runtime() -> NativeRuntime {
        NativeRuntime::new()
    }

    // exec_command/exec_pipe spawn the program directly with no shell, so these
    // tests must name a program that exists on the platform. Unix coreutils
    // (echo/false/cat) are not standalone executables on a stock Windows box, so
    // Windows routes through cmd.exe / findstr.exe (both always present in
    // System32). The runtime is byte-clean; the Windows echo program appends
    // CRLF, which is why the stdin round-trip assertions trim trailing newlines.
    #[cfg(not(windows))]
    fn echo_cmd() -> (&'static str, &'static [&'static str]) {
        ("echo", &["hello"])
    }
    #[cfg(windows)]
    fn echo_cmd() -> (&'static str, &'static [&'static str]) {
        ("cmd", &["/C", "echo", "hello"])
    }

    #[cfg(not(windows))]
    fn false_cmd() -> (&'static str, &'static [&'static str]) {
        ("false", &[])
    }
    #[cfg(windows)]
    fn false_cmd() -> (&'static str, &'static [&'static str]) {
        ("cmd", &["/C", "exit", "1"])
    }

    #[cfg(not(windows))]
    fn cat_cmd() -> (&'static str, &'static [&'static str]) {
        ("cat", &[])
    }
    #[cfg(windows)]
    fn cat_cmd() -> (&'static str, &'static [&'static str]) {
        ("findstr", &["^"])
    }

    #[test]
    fn test_file_read_write() {
        let temp = TempFileTempDir::new().unwrap();
        let path = temp.path().join("test.txt");
        let rt = runtime();

        // Write
        rt.file_write(&path, b"hello world").unwrap();
        assert!(path.exists());

        // Read
        let content = rt.file_read(&path).unwrap();
        assert_eq!(content, b"hello world");

        // Read as string
        let content_str = rt.file_read_string(&path).unwrap();
        assert_eq!(content_str, "hello world");
    }

    #[test]
    fn test_file_write_creates_parent_dirs() {
        let temp = TempFileTempDir::new().unwrap();
        let path = temp.path().join("nested/dir/test.txt");
        let rt = runtime();

        rt.file_write(&path, b"content").unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_file_read_nonexistent() {
        let rt = runtime();
        let result = rt.file_read(Path::new("/nonexistent/path/file.txt"));
        assert!(result.is_err());
    }

    #[test]
    fn test_path_exists() {
        let temp = TempFileTempDir::new().unwrap();
        let file_path = temp.path().join("file.txt");
        let dir_path = temp.path().join("subdir");
        let rt = runtime();

        // Create file and directory
        fs::write(&file_path, "content").unwrap();
        fs::create_dir(&dir_path).unwrap();

        // Test exists without type filter
        assert!(rt.path_exists(&file_path, None).unwrap());
        assert!(rt.path_exists(&dir_path, None).unwrap());
        assert!(
            !rt.path_exists(&temp.path().join("nonexistent"), None)
                .unwrap()
        );

        // Test with type filter
        assert!(rt.path_exists(&file_path, Some(PathKind::File)).unwrap());
        assert!(
            !rt.path_exists(&file_path, Some(PathKind::Directory))
                .unwrap()
        );
        assert!(
            rt.path_exists(&dir_path, Some(PathKind::Directory))
                .unwrap()
        );
        assert!(!rt.path_exists(&dir_path, Some(PathKind::File)).unwrap());
    }

    #[test]
    fn test_path_metadata() {
        let temp = TempFileTempDir::new().unwrap();
        let file_path = temp.path().join("file.txt");
        let rt = runtime();

        fs::write(&file_path, "hello").unwrap();

        let metadata = rt.path_metadata(&file_path).unwrap();
        assert_eq!(metadata.kind, PathKind::File);
        assert_eq!(metadata.size, 5);
        assert!(metadata.modified.is_some());
    }

    #[test]
    fn test_file_copy() {
        let temp = TempFileTempDir::new().unwrap();
        let src = temp.path().join("src.txt");
        let dst = temp.path().join("dst.txt");
        let rt = runtime();

        fs::write(&src, "original content").unwrap();
        rt.file_copy(&src, &dst).unwrap();

        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "original content");
    }

    #[test]
    fn test_path_rename() {
        let temp = TempFileTempDir::new().unwrap();
        let old = temp.path().join("old.txt");
        let new = temp.path().join("new.txt");
        let rt = runtime();

        fs::write(&old, "content").unwrap();
        rt.path_rename(&old, &new).unwrap();

        assert!(!old.exists());
        assert!(new.exists());
        assert_eq!(fs::read_to_string(&new).unwrap(), "content");
    }

    #[test]
    fn test_file_remove() {
        let temp = TempFileTempDir::new().unwrap();
        let path = temp.path().join("to_remove.txt");
        let rt = runtime();

        fs::write(&path, "content").unwrap();
        assert!(path.exists());

        rt.file_remove(&path).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_dir_create_and_remove() {
        let temp = TempFileTempDir::new().unwrap();
        let dir = temp.path().join("newdir");
        let rt = runtime();

        // Create
        rt.dir_create(&dir, false).unwrap();
        assert!(dir.is_dir());

        // Remove
        rt.dir_remove(&dir, false).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn test_dir_create_recursive() {
        let temp = TempFileTempDir::new().unwrap();
        let dir = temp.path().join("a/b/c/d");
        let rt = runtime();

        rt.dir_create(&dir, true).unwrap();
        assert!(dir.is_dir());
    }

    #[test]
    fn test_dir_remove_recursive() {
        let temp = TempFileTempDir::new().unwrap();
        let dir = temp.path().join("parent");
        let rt = runtime();

        fs::create_dir_all(dir.join("child")).unwrap();
        fs::write(dir.join("child/file.txt"), "content").unwrap();

        rt.dir_remove(&dir, true).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn test_dir_list() {
        let temp = TempFileTempDir::new().unwrap();
        let rt = runtime();

        fs::write(temp.path().join("a.txt"), "").unwrap();
        fs::write(temp.path().join("b.txt"), "").unwrap();
        fs::create_dir(temp.path().join("subdir")).unwrap();

        let entries = rt.dir_list(temp.path()).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn test_cwd() {
        let rt = runtime();
        let cwd = rt.cwd().unwrap();
        assert!(cwd.is_absolute());
        assert!(cwd.exists());
    }

    #[test]
    fn test_temp_dir() {
        let rt = runtime();
        let temp = rt.temp_dir("test_prefix").unwrap();

        assert!(temp.path().exists());
        assert!(temp.path().is_dir());
        assert!(temp.path().to_string_lossy().contains("test_prefix"));

        let path = temp.path().to_path_buf();
        drop(temp);

        // Should be cleaned up
        assert!(!path.exists());
    }

    #[test]
    fn test_exec_command_success() {
        let rt = runtime();

        let (cmd, args) = echo_cmd();
        let output = rt.exec_command(cmd, args, None).unwrap();

        assert!(output.success());
        assert!(output.stdout_string().contains("hello"));
    }

    #[test]
    fn test_exec_command_failure() {
        let rt = runtime();

        let (cmd, args) = false_cmd();
        let output = rt.exec_command(cmd, args, None).unwrap();

        assert!(!output.success());
    }

    #[test]
    fn test_exec_command_with_stdin() {
        let rt = runtime();

        let (cmd, args) = cat_cmd();
        let output = rt.exec_command(cmd, args, Some(b"input data")).unwrap();

        assert!(output.success());
        // trim_end: the Windows echo program (findstr) appends CRLF; the runtime
        // itself passes stdin through unchanged.
        assert_eq!(output.stdout_string().trim_end(), "input data");
    }

    #[test]
    fn test_exec_pipe_success() {
        let rt = runtime();

        let (cmd, args) = cat_cmd();
        let output = rt.exec_pipe(cmd, args, b"pipe input").unwrap();

        // trim_end: the Windows echo program (findstr) appends CRLF; the runtime
        // itself passes stdin through unchanged.
        assert_eq!(String::from_utf8_lossy(&output).trim_end(), "pipe input");
    }

    #[test]
    fn test_exec_pipe_failure() {
        let rt = runtime();

        let (cmd, args) = false_cmd();
        let result = rt.exec_pipe(cmd, args, b"");

        assert!(matches!(result, Err(RuntimeError::ProcessFailed { .. })));
    }

    #[test]
    fn test_env_get() {
        let rt = runtime();

        // PATH should exist on all systems
        let path = rt.env_get("PATH").unwrap();
        assert!(path.is_some());

        // Nonexistent variable
        let none = rt.env_get("DEFINITELY_NOT_A_REAL_VAR_12345").unwrap();
        assert!(none.is_none());
    }

    #[test]
    fn test_env_all() {
        let rt = runtime();

        let all = rt.env_all().unwrap();
        assert!(!all.is_empty());
        assert!(all.contains_key("PATH") || all.contains_key("Path"));
    }

    #[test]
    fn test_os_name() {
        let rt = runtime();
        let os = rt.os_name();

        // Should be a known OS
        assert!(
            [
                "linux",
                "macos",
                "windows",
                "freebsd",
                "openbsd",
                "netbsd",
                "dragonfly",
                "ios",
                "android"
            ]
            .contains(&os)
                || os == "darwin" // macOS reports as "darwin"
        );
    }

    #[test]
    fn test_arch() {
        let rt = runtime();
        let arch = rt.arch();

        // Should be a known architecture
        assert!(
            [
                "x86",
                "x86_64",
                "arm",
                "aarch64",
                "mips",
                "mips64",
                "powerpc",
                "powerpc64",
                "riscv64",
                "s390x",
                "wasm32"
            ]
            .contains(&arch)
        );
    }

    #[test]
    fn test_xdg_dir() {
        let rt = runtime();

        // These should return valid paths (though they may not exist)
        let config = rt.xdg_dir(XdgDirKind::Config, None).unwrap();
        assert!(
            config.to_string_lossy().contains("config")
                || config.to_string_lossy().contains("Config")
        );

        let data = rt.xdg_dir(XdgDirKind::Data, None).unwrap();
        assert!(!data.as_os_str().is_empty());

        // Test with subpath
        let config_sub = rt
            .xdg_dir(XdgDirKind::Config, Some(Path::new("myapp")))
            .unwrap();
        assert!(config_sub.ends_with("myapp"));
    }

    #[test]
    fn test_stdout_stderr_write() {
        let rt = runtime();

        // These should not error (though we can't easily verify the output)
        rt.stdout_write(b"test stdout\n").unwrap();
        rt.stderr_write(b"test stderr\n").unwrap();
    }

    #[test]
    fn test_is_file_and_is_dir() {
        let temp = TempFileTempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        let dir_path = temp.path().join("subdir");
        let rt = runtime();

        // Create file and directory
        fs::write(&file_path, "content").unwrap();
        fs::create_dir(&dir_path).unwrap();

        // Test is_file
        assert!(rt.is_file(&file_path).unwrap());
        assert!(!rt.is_file(&dir_path).unwrap());
        assert!(!rt.is_file(&temp.path().join("nonexistent")).unwrap());

        // Test is_dir
        assert!(!rt.is_dir(&file_path).unwrap());
        assert!(rt.is_dir(&dir_path).unwrap());
        assert!(!rt.is_dir(&temp.path().join("nonexistent")).unwrap());
    }

    #[test]
    fn test_find_binary() {
        let rt = runtime();

        // "echo" should exist on all Unix-like systems and Windows
        // (on Windows it's a shell builtin but `which` should find cmd.exe or similar)
        #[cfg(unix)]
        {
            let echo = rt.find_binary("echo", "NONEXISTENT_ENV_VAR");
            assert!(echo.is_some(), "echo binary should be found on PATH");
        }

        // Test with a definitely nonexistent binary
        let nonexistent =
            rt.find_binary("definitely_not_a_real_binary_12345", "NONEXISTENT_ENV_VAR");
        assert!(nonexistent.is_none());
    }

    #[test]
    fn test_canonicalize() {
        let temp = TempFileTempDir::new().unwrap();
        let file_path = temp.path().join("test.txt");
        let rt = runtime();

        // Create a file so canonicalize can work
        fs::write(&file_path, "content").unwrap();

        // Canonicalize should return an absolute path
        let canonical = rt.canonicalize(&file_path).unwrap();
        assert!(canonical.is_absolute());

        // Should fail for nonexistent paths
        let result = rt.canonicalize(&temp.path().join("nonexistent.txt"));
        assert!(result.is_err());
    }

    // SASS compilation tests (via trait methods)
    #[test]
    fn test_sass_available() {
        let rt = runtime();
        assert!(rt.sass_available());
    }

    #[test]
    fn test_sass_compiler_name() {
        let rt = runtime();
        assert_eq!(rt.sass_compiler_name(), Some("grass"));
    }

    #[test]
    fn svg_rasterization_is_unavailable_on_native() {
        let rt = runtime();
        assert!(!rt.can_rasterize_svg());
        let err = pollster::block_on(rt.rasterize_svg(b"<svg/>", 2048)).unwrap_err();
        assert!(matches!(err, RuntimeError::NotSupported(_)), "{err}");
    }

    #[test]
    fn test_compile_sass_via_trait() {
        let rt = runtime();
        let scss = "$color: #333; body { color: $color; }";

        let result = pollster::block_on(rt.compile_sass(scss, &[], false));

        assert!(result.is_ok());
        let css = result.unwrap().css;
        assert!(css.contains("body"));
        assert!(css.contains("#333"));
    }

    #[test]
    fn test_compile_sass_minified_via_trait() {
        let rt = runtime();
        let scss = ".container { margin: 0 auto; }";

        let result = pollster::block_on(rt.compile_sass(scss, &[], true));

        assert!(result.is_ok());
        let css = result.unwrap().css;
        // Minified output should not have newlines between selectors and braces
        assert!(css.contains(".container{") || css.contains(".container {"));
    }

    // ── Environment override ─────────────────────────────────────────────

    /// The override answers on its own instance only: a plain runtime
    /// (and the process environment) never sees it, and two overridden
    /// runtimes stay independent.
    #[test]
    fn test_env_override_is_instance_scoped() {
        use crate::traits::SystemRuntime;
        const NAME: &str = "Q2_TEST_ENV_OVERRIDE_INSTANCE_SCOPED";
        let a = NativeRuntime::new().with_env_override(NAME, "a");
        let b = NativeRuntime::new().with_env_override(NAME, "b");
        let plain = NativeRuntime::new();
        assert_eq!(a.env_get(NAME).unwrap().as_deref(), Some("a"));
        assert_eq!(b.env_get(NAME).unwrap().as_deref(), Some("b"));
        assert_eq!(plain.env_get(NAME).unwrap(), None);
        assert!(std::env::var(NAME).is_err(), "nothing set process-wide");
        assert_eq!(
            a.env_all().unwrap().get(NAME).map(String::as_str),
            Some("a")
        );
        assert!(!plain.env_all().unwrap().contains_key(NAME));
    }

    /// An unrelated name still reads the process environment, and a
    /// second override of the same name replaces the first.
    #[test]
    fn test_env_override_falls_through_and_replaces() {
        use crate::traits::SystemRuntime;
        let rt = NativeRuntime::new()
            .with_env_override("Q2_TEST_ENV_OVERRIDE_X", "1")
            .with_env_override("Q2_TEST_ENV_OVERRIDE_X", "2");
        assert_eq!(
            rt.env_get("Q2_TEST_ENV_OVERRIDE_X").unwrap().as_deref(),
            Some("2")
        );
        assert_eq!(
            rt.env_get("PATH").unwrap(),
            NativeRuntime::new().env_get("PATH").unwrap()
        );
    }

    // ── Cache: NativeRuntime construction ────────────────────────────────

    #[test]
    fn test_native_runtime_new_has_no_cache_dir() {
        let rt = NativeRuntime::new();
        assert!(rt.cache_dir().is_none());
    }

    /// `cache_list` enumerates a namespace's entries with sizes and
    /// mtimes, skips in-flight atomic-write temp files, reports an
    /// absent namespace as empty, and reports "cannot enumerate" when
    /// no cache dir is configured (bd-ddahjqr1).
    #[test]
    fn test_native_cache_list() {
        use crate::traits::SystemRuntime;
        let temp = tempfile::TempDir::new().unwrap();
        let rt = NativeRuntime::with_cache_dir(temp.path().to_path_buf());

        assert_eq!(
            pollster::block_on(rt.cache_list("sass")).unwrap(),
            Some(Vec::new()),
            "absent namespace lists as empty"
        );

        pollster::block_on(rt.cache_set("sass", "a", b"aaaa")).unwrap();
        pollster::block_on(rt.cache_set("sass", "bb", b"bbbbbbbb")).unwrap();
        fs::write(temp.path().join("sass").join(".tmpXYZ"), b"in-flight").unwrap();

        let mut listed = pollster::block_on(rt.cache_list("sass")).unwrap().unwrap();
        listed.sort_by(|x, y| x.key.cmp(&y.key));
        let summary: Vec<(&str, u64, bool)> = listed
            .iter()
            .map(|e| (e.key.as_str(), e.size, e.modified_ms.is_some()))
            .collect();
        assert_eq!(summary, vec![("a", 4, true), ("bb", 8, true)]);

        let no_cache = NativeRuntime::new();
        assert_eq!(
            pollster::block_on(no_cache.cache_list("sass")).unwrap(),
            None,
            "no cache dir → cannot enumerate"
        );
    }

    #[test]
    fn test_native_runtime_with_cache_dir() {
        let dir = PathBuf::from("/tmp/test-cache");
        let rt = NativeRuntime::with_cache_dir(dir.clone());
        assert_eq!(rt.cache_dir(), Some(dir.as_path()));
    }

    // ── Cache: default trait impls via SandboxedRuntime ──────────────────

    #[test]
    fn test_cache_defaults_get_returns_none() {
        use crate::sandbox::{SandboxedRuntime, SecurityPolicy};
        let inner = NativeRuntime::new();
        let rt = SandboxedRuntime::new(inner, SecurityPolicy::trusted());
        let result = pollster::block_on(rt.cache_get("sass", "abc123"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_cache_defaults_set_is_noop() {
        use crate::sandbox::{SandboxedRuntime, SecurityPolicy};
        let inner = NativeRuntime::new();
        let rt = SandboxedRuntime::new(inner, SecurityPolicy::trusted());
        let result = pollster::block_on(rt.cache_set("sass", "abc123", b"data"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_defaults_delete_is_noop() {
        use crate::sandbox::{SandboxedRuntime, SecurityPolicy};
        let inner = NativeRuntime::new();
        let rt = SandboxedRuntime::new(inner, SecurityPolicy::trusted());
        let result = pollster::block_on(rt.cache_delete("sass", "abc123"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_defaults_clear_namespace_is_noop() {
        use crate::sandbox::{SandboxedRuntime, SecurityPolicy};
        let inner = NativeRuntime::new();
        let rt = SandboxedRuntime::new(inner, SecurityPolicy::trusted());
        let result = pollster::block_on(rt.cache_clear_namespace("sass"));
        assert!(result.is_ok());
    }

    // ── Cache: NativeRuntime filesystem implementation ───────────────────

    fn cache_runtime() -> (NativeRuntime, TempFileTempDir) {
        let temp = TempFileTempDir::new().unwrap();
        let rt = NativeRuntime::with_cache_dir(temp.path().to_path_buf());
        (rt, temp)
    }

    #[test]
    fn test_cache_roundtrip() {
        let (rt, _tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("sass", "abc123", b"css-content")).unwrap();
        let result = pollster::block_on(rt.cache_get("sass", "abc123")).unwrap();
        assert_eq!(result, Some(b"css-content".to_vec()));
    }

    #[test]
    fn test_cache_get_missing() {
        let (rt, _tmp) = cache_runtime();
        let result = pollster::block_on(rt.cache_get("sass", "nonexistent")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_get_no_cache_dir() {
        let rt = NativeRuntime::new();
        let result = pollster::block_on(rt.cache_get("sass", "abc123")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_set_no_cache_dir() {
        let rt = NativeRuntime::new();
        let result = pollster::block_on(rt.cache_set("sass", "abc123", b"data"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_overwrite() {
        let (rt, _tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("sass", "key1", b"first")).unwrap();
        pollster::block_on(rt.cache_set("sass", "key1", b"second")).unwrap();
        let result = pollster::block_on(rt.cache_get("sass", "key1")).unwrap();
        assert_eq!(result, Some(b"second".to_vec()));
    }

    #[test]
    fn test_cache_delete() {
        let (rt, _tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("sass", "key1", b"data")).unwrap();
        pollster::block_on(rt.cache_delete("sass", "key1")).unwrap();
        let result = pollster::block_on(rt.cache_get("sass", "key1")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_cache_delete_nonexistent() {
        let (rt, _tmp) = cache_runtime();
        let result = pollster::block_on(rt.cache_delete("sass", "nonexistent"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_clear_namespace() {
        let (rt, _tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("sass", "key1", b"a")).unwrap();
        pollster::block_on(rt.cache_set("sass", "key2", b"b")).unwrap();
        pollster::block_on(rt.cache_clear_namespace("sass")).unwrap();
        assert!(
            pollster::block_on(rt.cache_get("sass", "key1"))
                .unwrap()
                .is_none()
        );
        assert!(
            pollster::block_on(rt.cache_get("sass", "key2"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_cache_clear_nonexistent_namespace() {
        let (rt, _tmp) = cache_runtime();
        let result = pollster::block_on(rt.cache_clear_namespace("nonexistent"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_cache_namespaces_isolated() {
        let (rt, _tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("ns1", "key1", b"value-a")).unwrap();
        pollster::block_on(rt.cache_set("ns2", "key1", b"value-b")).unwrap();
        assert_eq!(
            pollster::block_on(rt.cache_get("ns1", "key1")).unwrap(),
            Some(b"value-a".to_vec())
        );
        assert_eq!(
            pollster::block_on(rt.cache_get("ns2", "key1")).unwrap(),
            Some(b"value-b".to_vec())
        );
    }

    #[test]
    fn test_cache_invalid_key_rejected() {
        let (rt, _tmp) = cache_runtime();
        assert!(pollster::block_on(rt.cache_get("sass", "bad/key")).is_err());
        assert!(pollster::block_on(rt.cache_get("sass", "..")).is_err());
        assert!(pollster::block_on(rt.cache_set("sass", "bad/key", b"x")).is_err());
    }

    #[test]
    fn test_cache_invalid_namespace_rejected() {
        let (rt, _tmp) = cache_runtime();
        assert!(pollster::block_on(rt.cache_get("bad/ns", "key1")).is_err());
        assert!(pollster::block_on(rt.cache_get("..", "key1")).is_err());
        assert!(pollster::block_on(rt.cache_set("bad/ns", "key1", b"x")).is_err());
    }

    #[test]
    fn test_cache_empty_value() {
        let (rt, _tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("sass", "empty", b"")).unwrap();
        let result = pollster::block_on(rt.cache_get("sass", "empty")).unwrap();
        assert_eq!(result, Some(vec![]));
    }

    #[test]
    fn test_cache_large_value() {
        let (rt, _tmp) = cache_runtime();
        let large = vec![0xABu8; 1_000_000];
        pollster::block_on(rt.cache_set("sass", "large", &large)).unwrap();
        let result = pollster::block_on(rt.cache_get("sass", "large")).unwrap();
        assert_eq!(result, Some(large));
    }

    #[test]
    fn test_cache_binary_value() {
        let (rt, _tmp) = cache_runtime();
        let binary: Vec<u8> = (0..=255).collect();
        pollster::block_on(rt.cache_set("bin", "all-bytes", &binary)).unwrap();
        let result = pollster::block_on(rt.cache_get("bin", "all-bytes")).unwrap();
        assert_eq!(result, Some(binary));
    }

    #[test]
    fn test_cache_creates_directories() {
        let (rt, tmp) = cache_runtime();
        pollster::block_on(rt.cache_set("sass", "key1", b"data")).unwrap();
        // The namespace directory should have been created
        assert!(tmp.path().join("sass").is_dir());
        assert!(tmp.path().join("sass").join("key1").is_file());
    }

    #[test]
    fn is_interactive_native_false_under_nextest() {
        // nextest runs tests without a TTY, so stdin is not a terminal.
        // This asserts the thin IsTerminal call returns false in the test harness.
        // The `true` path requires a PTY and is not unit-tested (acceptable — it
        // is a one-liner delegating to std::io::IsTerminal).
        let rt = NativeRuntime::new();
        assert!(
            !rt.is_interactive(),
            "NativeRuntime::is_interactive() should be false under nextest (no PTY)"
        );
    }

    // ── fetch_url ──────────────────────────────────────────────────────────
    //
    // A loopback TCP listener stands in for the server. The handler gets the
    // accepted stream after the request line has been read, so each test
    // scripts exactly one response (or none, for the stall).

    fn serve_once(
        respond: impl FnOnce(&mut std::net::TcpStream) + Send + 'static,
    ) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{BufRead, BufReader};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            // Consume the request head so the client is not blocked writing it.
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 && line.trim() != "" {
                line.clear();
            }
            respond(&mut stream);
        });
        (format!("http://{addr}"), handle)
    }

    fn respond_with(
        stream: &mut std::net::TcpStream,
        status: &str,
        content_type: &str,
        body: &str,
    ) {
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body.as_bytes()).unwrap();
        stream.flush().unwrap();
    }

    #[test]
    fn fetch_url_returns_body_and_content_type() {
        let (base, server) = serve_once(|s| respond_with(s, "200 OK", "text/css", "body{}"));
        let (bytes, mime) =
            pollster::block_on(runtime().fetch_url(&format!("{base}/a.css"))).unwrap();
        assert_eq!(bytes, b"body{}");
        assert_eq!(mime, "text/css");
        server.join().unwrap();
    }

    #[test]
    fn fetch_url_treats_a_non_2xx_status_as_an_error() {
        let (base, server) = serve_once(|s| {
            respond_with(
                s,
                "503 Service Unavailable",
                "text/html",
                "<html>busy</html>",
            )
        });
        let url = format!("{base}/css2?family=Fira+Code");
        let err = pollster::block_on(runtime().fetch_url(&url)).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("503"), "{text}");
        assert!(text.contains(&url), "{text}");
        assert!(matches!(err, RuntimeError::Network(_)), "{err:?}");
        server.join().unwrap();
    }

    #[test]
    fn fetch_blocking_gives_up_on_a_stalled_connection() {
        // The server accepts, reads the request, and then goes quiet. The
        // sleep is longer than the client's timeout, so a client that waits
        // for the response would hang; dropping the stream afterwards lets
        // the thread finish.
        let (base, server) = serve_once(|_| std::thread::sleep(Duration::from_millis(1500)));
        let url = format!("{base}/slow");
        let err = fetch_blocking(&url, Duration::from_millis(200)).unwrap_err();
        // What proves the client gave up on its own is *which* error ends the
        // request: its own 200 ms timeout, not the connection reset from the
        // server dropping the stream when its silence ends. Assert on that
        // identity rather than wall-clock time — under full-suite load this
        // process can be descheduled for seconds at a time, and a hard
        // `elapsed < 1200 ms` bound flaked even though the timeout fired
        // correctly every time (bd-u6pt7ubh).
        let RuntimeError::Network(msg) = &err else {
            panic!("expected RuntimeError::Network, got {err:?}")
        };
        assert!(
            msg.contains("timed out after 200ms"),
            "expected the request's own 200 ms timeout to fire, got: {msg}"
        );
        server.join().unwrap();
    }

    #[test]
    fn fetch_url_hardened_refuses_plain_http() {
        let err = pollster::block_on(
            runtime().fetch_url_hardened("http://127.0.0.1:9/x.png", &FetchPolicy::default()),
        )
        .unwrap_err();
        assert!(matches!(err, RuntimeError::Network(_)), "{err:?}");
    }
}
